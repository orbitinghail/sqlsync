use std::io;
use std::net::TcpListener;
use std::net::TcpStream;
use std::net::ToSocketAddrs;
use std::sync::{Arc, Mutex};
use std::thread;

use bincode::ErrorKind;
use sqlsync::local::LocalDocument;
use sqlsync::replication::ReplicationMsg;
use sqlsync::replication::ReplicationProtocol;
use sqlsync::{Journal, JournalId};

use serde::{Deserialize, Serialize};
use sqlsync::{coordinator::CoordinatorDocument, MemoryJournal};

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

fn send_msg(socket: &mut TcpStream, msg: &ReplicationMsg) -> io::Result<()> {
    serialize_into(socket, msg)
}

fn receive_msg(socket: &mut TcpStream) -> io::Result<ReplicationMsg> {
    deserialize_from(socket)
}

#[derive(Serialize, Deserialize, Debug)]
enum Mutation {
    InitSchema,
    Incr,
    Decr,
}

fn start_server<'a>(
    listener: TcpListener,
    doc_id: JournalId,
    expected_clients: usize,
    thread_scope: &'a thread::Scope<'a, '_>,
) -> anyhow::Result<()> {
    let wasm_bytes = include_bytes!(
        "../../../target/wasm32-unknown-unknown/debug/examples/counter_reducer.wasm"
    );

    // build a ServerDocument and protect it with a mutex since multiple threads will be accessing it
    let storage_journal = MemoryJournal::open(doc_id)?;
    let coordinator = CoordinatorDocument::open(storage_journal, &wasm_bytes[..])?;
    let coordinator = Arc::new(Mutex::new(coordinator));

    for _ in 0..expected_clients {
        log::info!("server: waiting for client connection");
        let (socket, _) = listener.accept()?;
        let doc = coordinator.clone();
        thread_scope.spawn(move || match handle_client(doc, socket) {
            Ok(()) => {}
            Err(e) => {
                // handle eof
                match e.root_cause().downcast_ref::<io::Error>() {
                    Some(err)
                        if err.kind() == io::ErrorKind::UnexpectedEof
                            || err.kind() == io::ErrorKind::ConnectionReset =>
                    {
                        log::info!("handle_client: client disconnected");
                        return;
                    }
                    _ => {}
                }

                log::error!("handle_client failed: {:?}", e);
            }
        });
    }

    Ok(())
}

fn handle_client(
    doc: Arc<Mutex<CoordinatorDocument<MemoryJournal>>>,
    mut socket: TcpStream,
) -> anyhow::Result<()> {
    log::info!("server: received client connection");
    let mut protocol = ReplicationProtocol::new();

    macro_rules! unlock {
        (|$doc:ident| $block:block) => {{
            let mut guard = $doc.lock().expect("poisoned lock");
            let $doc = &mut *guard;
            $block
        }};

        (|$doc:ident| $expr:expr) => {{
            unlock!(|$doc| { $expr })
        }};
    }

    // send start message
    send_msg(&mut socket, unlock!(|doc| &protocol.start(doc)))?;

    // handle start from client
    while !protocol.initialized() {
        let msg = receive_msg(&mut socket)?;
        log::info!("server: received {:?}", msg);
        if let Some(resp) = unlock!(|doc| protocol.handle(doc, msg, &mut socket)?) {
            log::info!("server: sending {:?}", resp);
            send_msg(&mut socket, &resp)?;
        }
    }

    log::info!("server: initialized connection to client");

    let mut num_steps = 0;

    loop {
        let msg = receive_msg(&mut socket)?;
        log::info!("server: received {:?}", msg);

        if let Some(resp) = unlock!(|doc| protocol.handle(doc, msg, &mut socket)?) {
            log::info!("server: sending {:?}", resp);
            send_msg(&mut socket, &resp)?;
        }

        // step after every message
        num_steps += 1;
        log::info!("server: stepping doc (steps: {})", num_steps);
        unlock!(|doc| doc.step()?);

        // sync back to the client if needed
        unlock!(|doc| {
            if let Some((msg, mut reader)) = protocol.sync(doc)? {
                log::info!("server: syncing to client: {:?}", msg);
                send_msg(&mut socket, &msg)?;
                // write the frame
                io::copy(&mut reader, &mut socket)?;
            }
        });
    }
}

fn start_client(
    addr: impl ToSocketAddrs,
    num_clients: usize,
    doc_id: JournalId,
) -> anyhow::Result<()> {
    let mut socket = TcpStream::connect(addr)?;

    let wasm_bytes = include_bytes!(
        "../../../target/wasm32-unknown-unknown/debug/examples/counter_reducer.wasm"
    );

    // generate random timeline id and open doc
    let timeline_id = JournalId::new();
    let timeline_journal = MemoryJournal::open(timeline_id)?;
    let storage_journal = MemoryJournal::open(doc_id)?;
    let mut doc = LocalDocument::open(storage_journal, timeline_journal, &wasm_bytes[..])?;

    // initialize schema
    doc.mutate(&bincode::serialize(&Mutation::InitSchema)?)?;

    let mut protocol = ReplicationProtocol::new();

    // send start message
    send_msg(&mut socket, &protocol.start(&mut doc))?;

    // handle start from server
    let msg = receive_msg(&mut socket)?;
    log::info!("client({}): received {:?}", timeline_id, msg);

    if let Some(resp) = protocol.handle(&mut doc, msg, &mut socket)? {
        log::info!("client({}): sending {:?}", timeline_id, resp);
        send_msg(&mut socket, &resp)?;
    }

    log::info!("client({}): initialized connection to server", timeline_id);

    // the amount of mutations we will send the server
    let total_mutations = 10 as usize;
    let mut remaining_mutations = total_mutations;

    // switch to nonblocking mode for the core client loop
    socket.set_nonblocking(true)?;

    loop {
        loop {
            let msg = match receive_msg(&mut socket) {
                Ok(msg) => msg,
                Err(err) => {
                    match err.kind() {
                        io::ErrorKind::WouldBlock => {
                            // no more messages
                            break;
                        }
                        _ => return Err(err.into()),
                    }
                }
            };
            log::info!("client({}): received {:?}", timeline_id, msg);

            if let Some(resp) = protocol.handle(&mut doc, msg, &mut socket)? {
                log::info!("client({}): sending {:?}", timeline_id, resp);
                send_msg(&mut socket, &resp)?;
            }
        }

        // trigger a rebase if needed
        doc.rebase()?;

        if remaining_mutations > 0 {
            log::info!("client({}): running incr", timeline_id);
            doc.mutate(&bincode::serialize(&Mutation::Incr)?)?;
            remaining_mutations -= 1;
        }

        // sync pending mutations to the server
        if protocol.initialized() {
            if let Some((msg, mut reader)) = protocol.sync(&mut doc)? {
                log::info!("client({}): syncing to server", timeline_id);
                send_msg(&mut socket, &msg)?;
                // write the frame
                io::copy(&mut reader, &mut socket)?;
            }
        }

        let mut all_mutations_applied = false;
        log::info!("client({}): QUERYING STATE", timeline_id);
        doc.query(|tx| {
            tx.query_row("select value from counter", [], |row| {
                let value: Option<i32> = row.get(0)?;
                log::info!("client({}): counter value: {:?}", timeline_id, value);
                Ok(())
            })?;

            let mut stmt = tx.prepare("select lsn from __sqlsync_timelines")?;
            let mut num_rows = 0;
            let mut iter = stmt.query_map([], |r| {
                num_rows += 1;
                Ok(r.get::<_, usize>(0)?)
            })?;
            all_mutations_applied = iter.all(|x| x == Ok(total_mutations)) && num_rows > 0;

            Ok(())
        })?;

        if all_mutations_applied {
            break;
        }

        // sleep a bit
        std::thread::sleep(std::time::Duration::from_millis(100));
    }

    // final query, value should be total_mutations * num_clients
    doc.query(|tx| {
        tx.query_row_and_then("select value from counter", [], |row| {
            let value: Option<usize> = row.get(0)?;
            log::info!("client({}): counter value: {:?}", timeline_id, value);
            if value != Some(total_mutations * num_clients) {
                return Err(anyhow::anyhow!(
                    "client({}): counter value is incorrect: {:?}, expected {}",
                    timeline_id,
                    value,
                    total_mutations * num_clients
                ));
            }
            Ok(())
        })?;
        Ok(())
    })?;

    log::info!("client({}): closing connection", timeline_id);

    Ok(())
}

fn main() -> anyhow::Result<()> {
    simple_logger::SimpleLogger::new()
        .with_level(log::LevelFilter::Debug)
        .env()
        .init()?;

    let addr = "127.0.0.1:8080";
    let listener = TcpListener::bind(addr)?;
    let doc_id = JournalId::new();

    thread::scope(|s| {
        let server_scope = s.clone();
        let num_clients = 2;

        s.spawn(move || {
            start_server(listener, doc_id, num_clients, server_scope).expect("server failed")
        });

        for _ in 0..num_clients {
            s.spawn(move || start_client(addr, num_clients, doc_id).expect("client failed"));
        }
    });

    Ok(())
}
