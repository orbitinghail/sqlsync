use std::io;
use std::net::TcpListener;
use std::net::TcpStream;
use std::net::ToSocketAddrs;
use std::sync::MutexGuard;
use std::sync::{Arc, Mutex};
use std::thread;

use bincode::ErrorKind;
use rand::thread_rng;
use rand::Rng;
use sqlsync::local::LocalDocument;
use sqlsync::Cursor;
use sqlsync::JournalPartial;
use sqlsync::RequestedLsnRange;
use sqlsync::Syncable;
use sqlsync::{Journal, JournalId, LsnRange};

use serde::{Deserialize, Serialize};
use sqlsync::{coordinator::CoordinatorDocument, positioned_io::PositionedReader, MemoryJournal};

const DOC_ID: JournalId = 1;

// TODO: this should be smarter, syncing 5 journal frames is not optimal in many cases
const DEFAULT_REQUEST_LEN: usize = 5;

fn serialize_into<W, T: ?Sized>(writer: W, value: &T) -> io::Result<()>
where
    W: std::io::Write,
    T: serde::Serialize,
{
    match bincode::serialize_into(writer, value) {
        Ok(_) => Ok(()),
        Err(err) => match err.as_ref() {
            ErrorKind::Io(err) => Err(err.kind().into()),
            _ => Err(io::Error::new(io::ErrorKind::Other, err)),
        },
    }
}

fn deserialize_from<R, T>(reader: R) -> io::Result<T>
where
    R: std::io::Read,
    T: serde::de::DeserializeOwned,
{
    match bincode::deserialize_from(reader) {
        Ok(v) => Ok(v),
        Err(err) => match err.as_ref() {
            ErrorKind::Io(err) => Err(err.kind().into()),
            _ => Err(io::Error::new(io::ErrorKind::Other, err)),
        },
    }
}

#[derive(Serialize, Deserialize, Debug)]
enum Mutation {
    InitSchema,
    Incr,
    Decr,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
enum NetMsg {
    HelloRequest {
        timeline_id: JournalId,
        storage_range: RequestedLsnRange,
    },
    HelloResponse {
        timeline_range: RequestedLsnRange,
    },
    SyncStart {
        journal_id: JournalId,
        range: LsnRange,
        frames: usize,
    },
    SyncStartAck,
    SyncFrame {
        len: usize,
    },
    SyncAck {
        range: LsnRange,
    },
}

fn send_bincode(socket: &mut TcpStream, msg: &NetMsg) -> io::Result<()> {
    serialize_into(socket, msg)
}

fn receive_bincode(socket: &mut TcpStream) -> io::Result<NetMsg> {
    deserialize_from(socket)
}

fn protocol_hello_send(
    socket: &mut TcpStream,
    doc: &mut impl Syncable,
) -> anyhow::Result<RequestedLsnRange> {
    // send hello request
    send_bincode(
        socket,
        &NetMsg::HelloRequest {
            timeline_id: doc.source_id(),
            storage_range: doc.sync_request(DOC_ID)?,
        },
    )?;

    // wait for hello response
    let msg = receive_bincode(socket)?;
    match msg {
        NetMsg::HelloResponse { timeline_range } => Ok(timeline_range),
        _ => anyhow::bail!("expected HelloResponse, got {:?}", msg),
    }
}

fn protocol_hello_receive(
    socket: &mut TcpStream,
    doc: &mut MutexGuard<'_, impl Syncable>,
) -> anyhow::Result<RequestedLsnRange> {
    // wait for hello request
    let msg = receive_bincode(socket)?;
    match msg {
        NetMsg::HelloRequest {
            timeline_id,
            storage_range,
        } => {
            // send hello response
            send_bincode(
                socket,
                &NetMsg::HelloResponse {
                    timeline_range: doc.sync_request(timeline_id)?,
                },
            )?;

            Ok(storage_range)
        }
        _ => anyhow::bail!("expected HelloRequest, got {:?}", msg),
    }
}

fn protocol_sync_send(
    socket: &mut TcpStream,
    partial: JournalPartial<impl Cursor + PositionedReader>,
) -> anyhow::Result<LsnRange> {
    // announce that we are starting a Sync
    send_bincode(
        socket,
        &NetMsg::SyncStart {
            journal_id: partial.id(),
            range: partial.range(),
            frames: partial.len(),
        },
    )?;

    // XXX: it's possible that the other side concurrently sends a SyncStart, for now we just panic
    assert!(
        matches!(receive_bincode(socket)?, NetMsg::SyncStartAck),
        "detected concurrent send"
    );

    // send each frame
    let mut cursor = partial.into_read_partial().into_cursor();
    while cursor.advance()? {
        let len = cursor.size()?;
        send_bincode(socket, &NetMsg::SyncFrame { len })?;

        io::copy(&mut cursor, socket)?;
    }

    // wait for the other side to acknowledge the sync
    let msg = receive_bincode(socket)?;
    match msg {
        NetMsg::SyncAck { range } => Ok(range),
        _ => anyhow::bail!("expected SyncAck, got {:?}", msg),
    }
}

struct SyncReceiveCursor<'a> {
    socket: &'a mut TcpStream,
    frames: usize,

    // current frame info
    pos: usize,
    len: usize,
}

impl<'a> SyncReceiveCursor<'a> {
    fn new(socket: &'a mut TcpStream, frames: usize) -> Self {
        Self {
            socket,
            frames,
            pos: 0,
            len: 0,
        }
    }
}

impl<'a> Cursor for SyncReceiveCursor<'a> {
    fn advance(&mut self) -> io::Result<bool> {
        if self.frames == 0 {
            return Ok(false);
        }

        // error if the previous frame wasn't fully consumed
        if self.pos != self.len {
            return Err(
                io::Error::new(io::ErrorKind::Other, "previous frame was not consumed").into(),
            );
        }

        let msg = receive_bincode(self.socket)?;
        match msg {
            NetMsg::SyncFrame { len } => {
                self.frames -= 1;
                self.pos = 0;
                self.len = len;
                Ok(true)
            }
            _ => Err(io::Error::new(io::ErrorKind::Other, "expected SyncFrame, got {:?}").into()),
        }
    }

    fn remaining(&self) -> usize {
        self.frames
    }
}

impl<'a> io::Read for SyncReceiveCursor<'a> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let limit = self.len - self.pos;
        if limit == 0 {
            return Ok(0);
        }

        let max = limit.min(buf.len());
        let n = self.socket.read(&mut buf[..max])?;
        assert!(n <= limit, "read more than expected");

        // update frame_remaining
        self.pos += n;

        Ok(n)
    }
}

fn protocol_sync_receive<'a>(
    socket: &'a mut TcpStream,
    sync_start: NetMsg,
) -> anyhow::Result<JournalPartial<SyncReceiveCursor<'a>>> {
    if let NetMsg::SyncStart {
        journal_id,
        range,
        frames,
    } = sync_start
    {
        log::info!(
            "sync_receive: journal {} with {} frames and {:?} range",
            journal_id,
            frames,
            range
        );

        // send a SyncStartAck
        send_bincode(socket, &NetMsg::SyncStartAck)?;

        // return iterator over frames
        Ok(JournalPartial::new(
            journal_id,
            range,
            SyncReceiveCursor::new(socket, frames),
        ))
    } else {
        anyhow::bail!("expected SyncStart, got {:?}", sync_start)
    }
}

fn start_server(listener: TcpListener) -> anyhow::Result<()> {
    let wasm_bytes = include_bytes!(
        "../../../target/wasm32-unknown-unknown/debug/examples/counter_reducer.wasm"
    );

    // build a ServerDocument and protect it with a mutex since multiple threads will be accessing it
    let storage_journal = MemoryJournal::open(DOC_ID)?;
    let coordinator = CoordinatorDocument::open(storage_journal, &wasm_bytes[..])?;
    let coordinator = Arc::new(Mutex::new(coordinator));

    loop {
        let (socket, _) = listener.accept()?;
        let doc = coordinator.clone();
        thread::spawn(move || match handle_client(doc, socket) {
            Ok(()) => {}
            Err(e) => {
                log::error!("handle_client failed: {:?}", e);
            }
        });
    }
}

fn handle_client(
    doc: Arc<Mutex<CoordinatorDocument<MemoryJournal>>>,
    mut socket: TcpStream,
) -> anyhow::Result<()> {
    log::info!("server: received client connection");
    let mut client_storage_req = {
        let mut doc = doc.lock().expect("poisoned lock");
        protocol_hello_receive(&mut socket, &mut doc)?
    };
    log::info!(
        "server: completed hello, client requests storage range: {:?}",
        client_storage_req
    );

    // our demo server is very simple
    // every time we loop, first we block on the next sync request from the client
    // then after we receive that, we check to see if we have anything to send
    loop {
        log::info!("server: waiting for sync request from client");
        let msg = receive_bincode(&mut socket)?;
        match msg {
            msg @ NetMsg::SyncStart { .. } => {
                let mut doc = doc.lock().expect("poisoned lock");
                let partial = protocol_sync_receive(&mut socket, msg)?;
                let timeline_id = partial.id();
                let range = doc.sync_receive(partial)?;
                send_bincode(&mut socket, &NetMsg::SyncAck { range })?;
                log::info!(
                    "server: received sync from client {}, new range: {:?}",
                    timeline_id,
                    range
                );
            }
            msg => todo!("handle {:?}", msg),
        }

        // step the server
        {
            let mut doc = doc.lock().expect("poisoned lock");
            log::info!("server: stepping doc");
            doc.step()?
        }

        // check to see if we have pending changes, and if so, send them
        log::info!("server: checking for pending changes");
        {
            let mut doc = doc.lock().expect("poisoned lock");
            if let Some(partial) = doc.sync_prepare(client_storage_req)? {
                log::info!("server: syncing storage {:?} to client", partial.range());
                let range = protocol_sync_send(&mut socket, partial)?;
                client_storage_req = range.request_next(DEFAULT_REQUEST_LEN);
                log::info!("server: received sync response {:?}", range);
            }
        }

        // sleep a bit
        std::thread::sleep(std::time::Duration::from_secs(1));
    }
}

fn start_client(addr: impl ToSocketAddrs) -> anyhow::Result<()> {
    let mut socket = TcpStream::connect(addr)?;

    let wasm_bytes = include_bytes!(
        "../../../target/wasm32-unknown-unknown/debug/examples/counter_reducer.wasm"
    );

    // generate random timeline id and open doc
    let timeline_id: JournalId = thread_rng().gen::<u8>().into();
    let timeline_journal = MemoryJournal::open(timeline_id)?;
    let storage_journal = MemoryJournal::open(DOC_ID)?;
    let mut doc = LocalDocument::open(storage_journal, timeline_journal, &wasm_bytes[..])?;

    // initialize schema
    doc.mutate(&bincode::serialize(&Mutation::InitSchema)?)?;

    // begin hello protocol
    log::info!("client({}): starting hello protocol", timeline_id);
    let mut server_timeline_req = protocol_hello_send(&mut socket, &mut doc)?;

    // our demo client is very simple
    // every time we loop, first we check to see if we have any pending mutations to send the server
    // then after syncing to the server we block until we receive a sync request back
    loop {
        // check to see if we have pending mutations, and if so, send them
        log::info!("client({}): checking for pending mutations", timeline_id);
        if let Some(partial) = doc.sync_prepare(server_timeline_req)? {
            log::info!(
                "client({}): syncing timeline {:?} to server",
                timeline_id,
                partial.range()
            );
            let range = protocol_sync_send(&mut socket, partial)?;
            server_timeline_req = range.request_next(DEFAULT_REQUEST_LEN);
            log::info!(
                "client({}): received sync response {:?}",
                timeline_id,
                range
            );
        }

        // randomly run a mutation
        {
            // randomly pick between Mutation::Incr and Mutation::Decr
            let mutation = if thread_rng().gen_bool(0.5) {
                Mutation::Incr
            } else {
                Mutation::Decr
            };
            log::info!("client({}): running mutation {:?}", timeline_id, mutation);
            doc.query(|tx| {
                let mut stmt = tx.prepare("select value from counter")?;
                let rows: Result<Vec<_>, _> = stmt
                    .query_map([], |row| Ok(row.get::<_, Option<i64>>(0)?))?
                    .collect();

                log::info!("client({}): counter values: {:?}", timeline_id, rows?);
                Ok(())
            })?;
            doc.mutate(&bincode::serialize(&mutation)?)?;
        }

        // wait for the server to send us a sync request
        log::info!(
            "client({}): waiting for sync request from server",
            timeline_id
        );
        match receive_bincode(&mut socket)? {
            msg @ NetMsg::SyncStart { .. } => {
                log::info!("client({}): received sync request {:?}", timeline_id, msg);
                let partial = protocol_sync_receive(&mut socket, msg)?;
                let range = doc.sync_receive(partial)?;
                send_bincode(&mut socket, &NetMsg::SyncAck { range })?;
            }
            msg => todo!("handle {:?}", msg),
        }

        {
            log::info!("client({}): QUERYING STATE", timeline_id);
            doc.query(|tx| {
                Ok(tx.query_row("select value from counter", [], |row| {
                    let value: Option<i32> = row.get(0)?;
                    log::info!("client({}): counter value: {:?}", timeline_id, value);
                    Ok(())
                })?)
            })?;
        }

        // sleep a bit
        std::thread::sleep(std::time::Duration::from_secs(1));
    }
}

fn main() -> anyhow::Result<()> {
    simple_logger::SimpleLogger::new()
        .with_level(log::LevelFilter::Debug)
        .env()
        .init()?;

    let addr = "127.0.0.1:8080";
    let listener = TcpListener::bind(addr)?;

    thread::scope(|s| {
        s.spawn(move || start_server(listener).expect("server failed"));
        s.spawn(move || start_client(addr).expect("client failed"));
    });

    Ok(())
}
