// this is needed due to an issue with Tsify emitting non-snake_case names without the correct annotations
#![allow(non_snake_case)]

use std::{
    fmt::Debug,
    io::{self, Cursor},
};

use anyhow::bail;
use futures::{
    stream::{Fuse, SplitSink, SplitStream},
    SinkExt, StreamExt,
};
use gloo::net::websocket::{futures::WebSocket, Message};
use serde::Serialize;
use sqlsync::{
    local::Signal,
    replication::{ReplicationDestination, ReplicationMsg, ReplicationProtocol, ReplicationSource},
};
use tsify::Tsify;

use crate::utils::Backoff;

// reconnect backoff starts at 10ms and doubles each time, up to 5s
const MIN_BACKOFF_MS: u32 = 10;
const MAX_BACKOFF_MS: u32 = 5000;

pub struct CoordinatorClient<S: Signal> {
    // while url is none, the state will always be disabled
    url: Option<String>,

    // we use an option here to work around rust ownership rules when we are
    // transitioning the state
    state: Option<ConnectionState>,

    state_changed: S,
}

impl<S: Signal> CoordinatorClient<S> {
    pub fn new(doc_url: Option<String>, state_changed: S) -> Self {
        let state = Some(doc_url.as_ref().map_or_else(
            || ConnectionState::Disabled,
            |_| ConnectionState::Disconnected {
                backoff: Backoff::new(MIN_BACKOFF_MS, MAX_BACKOFF_MS),
            },
        ));

        Self { url: doc_url, state, state_changed }
    }

    pub fn can_enable(&self) -> bool {
        self.url.is_some()
    }

    // SAFETY: poll, status, and handle can not be called concurrently on the same CoordinatorClient
    pub async fn poll(&mut self) -> ConnectionTask {
        match self.state {
            Some(ref mut state) => state.poll().await,
            None => unreachable!("CoordinatorClient: invalid concurrent call to poll"),
        }
    }

    // SAFETY: poll, status, and handle can not be called concurrently on the same CoordinatorClient
    pub fn status(&self) -> ConnectionStatus {
        match self.state {
            Some(ref state) => state.status(),
            None => unreachable!("CoordinatorClient: invalid concurrent call to status"),
        }
    }

    // SAFETY: poll, status, and handle can not be called concurrently on the same CoordinatorClient
    pub async fn handle<'a, R, D>(&mut self, doc: &'a mut D, task: ConnectionTask)
    where
        R: io::Read,
        D: ReplicationDestination + ReplicationSource<Reader<'a> = R>,
    {
        // load the current state and status
        let state = self
            .state
            .take()
            .expect("CoordinatorClient: invalid concurrent call to handle");
        let status = state.status();

        log::info!(
            "coordinator client: state {:?} is handling task {:?}",
            status,
            task,
        );

        // handle the task
        let state = state.handle(&self.url, doc, task).await;

        // get the new status and save the new state
        let new_status = state.status();
        self.state.replace(state);

        // if status changed, emit a signal
        if status != new_status {
            self.state_changed.emit();
        }
    }
}

pub enum ConnectionTask {
    Disable,
    Connect,
    Recv(ReplicationMsg, Cursor<Vec<u8>>),
    Sync,
    Error(anyhow::Error),
}

impl Debug for ConnectionTask {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConnectionTask::Disable => write!(f, "Disable"),
            ConnectionTask::Connect => write!(f, "Connect"),
            ConnectionTask::Recv(_, _) => write!(f, "Recv"),
            ConnectionTask::Sync => write!(f, "Sync"),
            ConnectionTask::Error(e) => write!(f, "Error({:?})", e),
        }
    }
}

enum ConnectionState {
    Disabled,
    Disconnected {
        backoff: Backoff,
    },
    Connecting {
        conn: CoordinatorConnection,
        backoff: Backoff,
    },
    Connected {
        conn: CoordinatorConnection,
    },
}

#[derive(Debug, Serialize, Tsify, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[tsify(into_wasm_abi)]
pub enum ConnectionStatus {
    Disabled,
    Disconnected,
    Connecting,
    Connected,
}

impl ConnectionState {
    fn status(&self) -> ConnectionStatus {
        match self {
            Self::Disabled => ConnectionStatus::Disabled,
            Self::Disconnected { .. } => ConnectionStatus::Disconnected,
            Self::Connecting { .. } => ConnectionStatus::Connecting,
            Self::Connected { .. } => ConnectionStatus::Connected,
        }
    }
}

impl ConnectionState {
    async fn poll(&mut self) -> ConnectionTask {
        match self {
            ConnectionState::Disabled => {
                // block forever, someone else will need to transition us to a different state
                futures::future::pending::<()>().await;
                unreachable!("ConnectionState should never be disabled")
            }
            ConnectionState::Disconnected { backoff } => {
                backoff.wait().await;
                ConnectionTask::Connect
            }
            ConnectionState::Connecting { conn, .. } => conn
                .recv()
                .await
                .map_or_else(ConnectionTask::Error, |(msg, buf)| {
                    ConnectionTask::Recv(msg, buf)
                }),
            ConnectionState::Connected { conn } => conn
                .recv()
                .await
                .map_or_else(ConnectionTask::Error, |(msg, buf)| {
                    ConnectionTask::Recv(msg, buf)
                }),
        }
    }

    async fn handle<'a, R, D>(
        self,
        url: &Option<String>,
        doc: &'a mut D,
        task: ConnectionTask,
    ) -> ConnectionState
    where
        R: io::Read,
        D: ReplicationDestination + ReplicationSource<Reader<'a> = R>,
    {
        use ConnectionState::*;
        use ConnectionTask::*;

        let url = if url.is_some() {
            url.as_ref().unwrap()
        } else {
            return Disabled;
        };

        macro_rules! handle_err {
            ($backoff:ident, $err:ident) => {{
                log::error!("connection error: {:?}", $err);
                $backoff.step();
                ConnectionState::Disconnected { $backoff }
            }};
            ($err:ident) => {{
                log::error!("connection error: {:?}", $err);
                ConnectionState::Disconnected {
                    backoff: Backoff::new(MIN_BACKOFF_MS, MAX_BACKOFF_MS),
                }
            }};
        }

        match (self, task) {
            // disabled ignores all tasks except for Connect
            (Disabled, Connect) => match CoordinatorConnection::open(url, doc).await {
                Ok(conn) => ConnectionState::Connecting {
                    conn,
                    backoff: Backoff::new(MIN_BACKOFF_MS, MAX_BACKOFF_MS),
                },
                Err(e) => handle_err!(e),
            },
            (s @ Disabled, _) => s,

            // the disable task universally disables
            (_, Disable) => Disabled,

            (Disconnected { mut backoff }, Connect) => {
                match CoordinatorConnection::open(url, doc).await {
                    Ok(conn) => ConnectionState::Connecting { conn, backoff },
                    Err(e) => handle_err!(backoff, e),
                }
            }

            (Disconnected { mut backoff }, Error(e)) => handle_err!(backoff, e),

            // ignore sync/recv
            (s @ Disconnected { .. }, Sync) => s,
            (s @ Disconnected { .. }, Recv(_, _)) => s,

            (s @ Connecting { .. }, Connect) => s,

            (Connecting { mut conn, mut backoff }, Recv(msg, buf)) => {
                if let Err(e) = conn.handle(doc, msg, buf).await {
                    return handle_err!(backoff, e);
                }

                if conn.initialized() {
                    // we have connected! need to perform an initial sync
                    match conn.sync(doc).await {
                        Ok(()) => Connected { conn },
                        Err(e) => handle_err!(backoff, e),
                    }
                } else {
                    Connecting { conn, backoff }
                }
            }

            // can't sync until we have completed the connection
            (s @ Connecting { .. }, Sync) => s,

            (Connecting { mut backoff, .. }, Error(e)) => {
                handle_err!(backoff, e)
            }

            (s @ Connected { .. }, Connect) => s,

            (Connected { mut conn }, Recv(msg, buf)) => match conn.handle(doc, msg, buf).await {
                Ok(()) => Connected { conn },
                Err(e) => handle_err!(e),
            },

            (Connected { mut conn }, Sync) => match conn.sync(doc).await {
                Ok(()) => Connected { conn },
                Err(e) => handle_err!(e),
            },

            (Connected { .. }, Error(e)) => handle_err!(e),
        }
    }
}

struct CoordinatorConnection {
    reader: Fuse<SplitStream<WebSocket>>,
    writer: SplitSink<WebSocket, Message>,
    protocol: ReplicationProtocol,
}

impl CoordinatorConnection {
    async fn open<D>(url: &str, doc: &D) -> anyhow::Result<CoordinatorConnection>
    where
        D: ReplicationSource,
    {
        log::info!("connecting to {}", url);
        let (mut writer, reader) = WebSocket::open(url)?.split();
        let reader = reader.fuse();
        let protocol = ReplicationProtocol::new();

        let start_msg = protocol.start(doc);
        log::info!("sending start message: {:?}", start_msg);
        let start_msg = bincode::serialize(&start_msg)?;
        writer.send(Message::Bytes(start_msg)).await?;

        Ok(CoordinatorConnection { reader, writer, protocol })
    }

    fn initialized(&self) -> bool {
        self.protocol.initialized()
    }

    async fn send(&mut self, msg: ReplicationMsg) -> anyhow::Result<()> {
        let msg = bincode::serialize(&msg)?;
        Ok(self.writer.send(Message::Bytes(msg)).await?)
    }

    async fn recv(&mut self) -> anyhow::Result<(ReplicationMsg, Cursor<Vec<u8>>)> {
        let msg = self.reader.select_next_some().await?;
        match msg {
            Message::Bytes(bytes) => {
                let mut buf = io::Cursor::new(bytes);
                Ok((bincode::deserialize_from(&mut buf)?, buf))
            }
            Message::Text(text) => {
                bail!("received unexpected text message: {:?}", text)
            }
        }
    }

    async fn handle<D>(
        &mut self,
        doc: &mut D,
        msg: ReplicationMsg,
        mut buf: Cursor<Vec<u8>>,
    ) -> anyhow::Result<()>
    where
        D: ReplicationDestination,
    {
        log::info!("received message: {:?}", msg);
        if let Some(resp) = self.protocol.handle(doc, msg, &mut buf)? {
            log::info!("sending response: {:?}", resp);
            self.send(resp).await?;
        }
        Ok(())
    }

    async fn sync<'a, R, D>(&mut self, doc: &'a mut D) -> anyhow::Result<()>
    where
        R: io::Read,
        D: ReplicationSource<Reader<'a> = R>,
    {
        while let Some((msg, mut reader)) = self.protocol.sync(doc)? {
            log::info!("sending message: {:?}", msg);

            let mut buf = io::Cursor::new(vec![]);
            bincode::serialize_into(&mut buf, &msg)?;
            io::copy(&mut reader, &mut buf)?;

            self.writer.send(Message::Bytes(buf.into_inner())).await?;
        }
        Ok(())
    }
}
