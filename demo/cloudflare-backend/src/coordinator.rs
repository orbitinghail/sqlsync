use std::{
    collections::BTreeMap,
    io::{self, Cursor},
};

use anyhow::{anyhow, bail};
use futures::{
    channel::mpsc::{self},
    select_biased,
    stream::{repeat, SelectAll, SplitSink, SplitStream},
    FutureExt, SinkExt, StreamExt,
};
use gloo::net::websocket::{futures::WebSocket, Message, WebSocketError};
use gloo::timers::future::TimeoutFuture;
use sqlsync::{
    coordinator::CoordinatorDocument,
    replication::{ReplicationMsg, ReplicationProtocol, ReplicationSource},
    MemoryJournal, MemoryJournalFactory, WasmReducer,
};
use worker::{console_error, console_log, Error, State};

use crate::{object_id_to_journal_id, persistence::Persistence};

type Document = CoordinatorDocument<MemoryJournal, WasmReducer>;

pub struct Coordinator {
    accept_queue: mpsc::Sender<WebSocket>,
}

impl Coordinator {
    pub async fn init(
        state: &State,
        reducer_bytes: Vec<u8>,
    ) -> worker::Result<(Coordinator, CoordinatorTask)> {
        let id = object_id_to_journal_id(state.id())?;
        let (accept_queue_tx, accept_queue_rx) = mpsc::channel(10);

        console_log!("creating new document with id {}", id);

        let mut storage = MemoryJournal::open(id).map_err(|e| Error::RustError(e.to_string()))?;

        // load the persistence layer
        let persistence = Persistence::init(state.storage()).await?;
        // replay any persisted frames into storage
        persistence.replay(id, &mut storage).await?;

        let doc = CoordinatorDocument::open(
            storage,
            MemoryJournalFactory,
            WasmReducer::new(reducer_bytes.as_slice())
                .map_err(|e| Error::RustError(e.to_string()))?,
        )
        .map_err(|e| Error::RustError(e.to_string()))?;

        Ok((
            Self { accept_queue: accept_queue_tx },
            CoordinatorTask {
                accept_queue: accept_queue_rx,
                persistence,
                doc,
            },
        ))
    }

    pub async fn accept(&mut self, socket: WebSocket) -> anyhow::Result<()> {
        Ok(self.accept_queue.send(socket).await?)
    }
}

pub struct CoordinatorTask {
    accept_queue: mpsc::Receiver<WebSocket>,
    persistence: Persistence,
    doc: Document,
}

impl CoordinatorTask {
    // into_task consumes the Coordinator and runs it as a task
    pub async fn into_task(mut self) {
        let mut clients: BTreeMap<usize, Client> = BTreeMap::new();
        let mut messages = SelectAll::new();
        let mut next_client_idx = 0;

        const STEP_MIN_MS: u32 = 100;
        let mut step_trigger = TimeoutFuture::new(STEP_MIN_MS).fuse();

        // NOTE TO CODE REVIEWERS:
        // `select_biased!` is full of foot guns (see: [1] and [2])
        // It's only safe if each branch follows these rules:
        //  - if ready, return value without awaiting
        //  - if not ready, await precisely once and then return value
        //
        // Critically: if it's possible to await twice during the execution of a
        // single future handled by select! - then it's possible for the future
        // to be dropped in an intermediate state.
        //
        // [1]: https://tomaka.medium.com/a-look-back-at-asynchronous-rust-d54d63934a1c
        // [2]: https://blog.yoshuawuyts.com/futures-concurrency-3/

        loop {
            select_biased! {
                // handle steps
                _ = step_trigger => {
                    // apply any pending changes to the document
                    if let Err(e) = self.step().await {
                        console_error!("error stepping: {:?}", e);
                        continue;
                    }

                    // persist document state to storage
                    if let Err(e) = self.persist().await {
                        console_error!("error persisting: {:?}", e);
                        continue;
                    }

                    // sync all clients
                    for (_, client) in clients.iter_mut() {
                        if let Err(e) = client.sync(&self.doc).await {
                            console_error!("error syncing: {:?}", e);
                            continue;
                        }
                    }
                },

                // handle new clients
                socket = self.accept_queue.select_next_some() => {
                    let (mut client, reader) = Client::init(socket);
                    if let Err(e) = client.start_replication(&self.doc).await {
                        console_error!("error starting replication: {:?}", e);
                        continue;
                    }
                    next_client_idx += 1;
                    let client_idx = next_client_idx;
                    clients.insert(client_idx, client);
                    messages.push(repeat(client_idx).zip(reader));
                },

                // handle messages from clients
                (client_idx, msg) = messages.select_next_some() => {
                    let client = match clients.get_mut(&client_idx) {
                        Some(client ) => client,
                        None => {
                            console_error!("received message from unknown client {}", client_idx);
                            continue;
                        }
                    };
                    if let Err(e) = client.handle_message(&mut self.doc, msg).await {
                        console_error!("error handling message from client {}: {:?}", client_idx, e);
                        // remove client; note, we don't have to remove the
                        // reader from messages because SelectAll handles that
                        // automatically
                        clients.remove(&client_idx);
                    } else {
                        // schedule a step whenever we receive messages from a client
                        step_trigger = TimeoutFuture::new(STEP_MIN_MS).fuse();
                    }
                },
            }
        }
    }

    async fn step(&mut self) -> anyhow::Result<()> {
        while self.doc.has_pending_work() {
            self.doc.step()?;
        }

        Ok(())
    }

    async fn persist(&mut self) -> anyhow::Result<()> {
        let mut next_lsn = self.persistence.expected_lsn();
        while let Some(frame) = self.doc.read_lsn(next_lsn)? {
            self.persistence
                .write_lsn(next_lsn, frame.to_owned())
                .await
                .map_err(|e| anyhow!(e.to_string()))?;
            next_lsn = self.persistence.expected_lsn();
        }

        Ok(())
    }
}

struct Client {
    protocol: ReplicationProtocol,
    writer: SplitSink<WebSocket, Message>,
}

impl Client {
    fn init(socket: WebSocket) -> (Self, SplitStream<WebSocket>) {
        let (writer, reader) = socket.split();
        let protocol = ReplicationProtocol::new();
        (Self { protocol, writer }, reader)
    }

    async fn start_replication(&mut self, doc: &Document) -> anyhow::Result<()> {
        let msg = self.protocol.start(doc);
        self.send_msg(msg).await
    }

    async fn sync(&mut self, doc: &Document) -> anyhow::Result<()> {
        while let Some((msg, mut frame)) = self.protocol.sync(doc)? {
            console_log!("sending message {:?}", msg);
            let mut buf = Cursor::new(vec![]);
            bincode::serialize_into(&mut buf, &msg)?;
            io::copy(&mut frame, &mut buf)?;
            self.writer.send(Message::Bytes(buf.into_inner())).await?;
        }

        Ok(())
    }

    async fn send_msg(&mut self, msg: ReplicationMsg) -> anyhow::Result<()> {
        let data = bincode::serialize(&msg)?;
        console_log!("sending message {:?}", msg);
        Ok(self.writer.send(Message::Bytes(data)).await?)
    }

    async fn handle_message(
        &mut self,
        doc: &mut Document,
        msg: Result<Message, WebSocketError>,
    ) -> anyhow::Result<()> {
        match msg {
            Ok(Message::Bytes(bytes)) => {
                let mut cursor = Cursor::new(bytes);
                let msg: ReplicationMsg = bincode::deserialize_from(&mut cursor)?;
                console_log!("received message {:?}", msg);
                if let Some(resp) = self.protocol.handle(doc, msg, &mut cursor)? {
                    self.send_msg(resp).await?;
                }
                Ok(())
            }

            Ok(Message::Text(_)) => {
                bail!("received unexpected text message")
            }

            Err(e) => {
                bail!("websocket error: {:?}", e)
            }
        }
    }
}
