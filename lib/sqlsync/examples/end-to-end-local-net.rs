use std::io;
use std::io::BufReader;
use std::net::TcpListener;
use std::net::TcpStream;
use std::net::ToSocketAddrs;
use std::sync::{Arc, Mutex};
use std::thread;

use bincode::ErrorKind;
use rand::thread_rng;
use rand::Rng;
use sqlsync::local::LocalDocument;
use sqlsync::replication::ReplicationMsg;
use sqlsync::replication::ReplicationProtocol;
use sqlsync::Lsn;
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

fn send_msg<W: io::Write>(socket: W, msg: &ReplicationMsg) -> io::Result<()> {
    serialize_into(socket, msg)
}

fn receive_msg<R: io::Read>(socket: R) -> io::Result<ReplicationMsg> {
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
    socket: TcpStream,
) -> anyhow::Result<()> {
    log::info!("server: received client connection");
    let mut protocol = ReplicationProtocol::new();

    let mut socket_reader = BufReader::new(&socket);
    let mut socket_writer = &socket;

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
    let start_msg = unlock!(|doc| protocol.start(doc));
    log::info!("server: sending {:?}", start_msg);
    send_msg(socket_writer, &start_msg)?;

    let mut num_steps = 0;

    loop {
        let msg = receive_msg(&mut socket_reader)?;
        log::info!("server: received {:?}", msg);

        if let Some(resp) = unlock!(|doc| protocol.handle(doc, msg, &mut socket_reader)?) {
            log::info!("server: sending {:?}", resp);
            send_msg(socket_writer, &resp)?;
        }

        // step after every message
        num_steps += 1;
        log::info!("server: stepping doc (steps: {})", num_steps);
        unlock!(|doc| doc.step()?);

        // sync back to the client if needed
        unlock!(|doc| {
            if let Some((msg, mut reader)) = protocol.sync(doc)? {
                log::info!("server: syncing to client: {:?}", msg);
                send_msg(socket_writer, &msg)?;
                let frame_len = reader.len() as u64;
                // write the frame
                let n = io::copy(&mut reader, &mut socket_writer)?;
                assert!(
                    n == frame_len,
                    "expected to write {} bytes, wrote {}",
                    frame_len,
                    n
                );
            }
        });
    }
}

fn start_client(
    addr: impl ToSocketAddrs,
    num_clients: usize,
    doc_id: JournalId,
) -> anyhow::Result<()> {
    let socket = TcpStream::connect(addr)?;
    let mut socket_reader = BufReader::new(&socket);
    let mut socket_writer = &socket;

    let wasm_bytes = include_bytes!(
        "../../../target/wasm32-unknown-unknown/debug/examples/counter_reducer.wasm"
    );

    // generate random timeline id and open doc
    let timeline_id = JournalId::new128();
    let timeline_journal = MemoryJournal::open(timeline_id)?;
    let storage_journal = MemoryJournal::open(doc_id)?;
    let mut doc = LocalDocument::open(storage_journal, timeline_journal, &wasm_bytes[..])?;

    // initialize schema
    doc.mutate(&bincode::serialize(&Mutation::InitSchema)?)?;

    let mut protocol = ReplicationProtocol::new();

    // send start message
    let start_msg = protocol.start(&mut doc);
    log::info!("client({}): sending {:?}", timeline_id, start_msg);
    send_msg(socket_writer, &start_msg)?;

    log::info!("client({}): connected to server", timeline_id);

    // the amount of mutations we will send the server
    let total_mutations = 10 as usize;
    let mut remaining_mutations = total_mutations;

    loop {
        let msg = receive_msg(&mut socket_reader)?;
        log::info!("client({}): received {:?}", timeline_id, msg);

        if let Some(resp) = protocol.handle(&mut doc, msg, &mut socket_reader)? {
            log::info!("client({}): sending {:?}", timeline_id, resp);
            send_msg(socket_writer, &resp)?;
        }

        // trigger a rebase if needed
        doc.rebase()?;

        if remaining_mutations > 0 {
            log::info!("client({}): running incr", timeline_id);
            doc.mutate(&bincode::serialize(&Mutation::Incr)?)?;
            remaining_mutations -= 1;
        }

        // sync pending mutations to the server
        if let Some((msg, mut reader)) = protocol.sync(&mut doc)? {
            log::info!("client({}): syncing to server: {:?}", timeline_id, msg);
            send_msg(socket_writer, &msg)?;
            // write the frame
            io::copy(&mut reader, &mut socket_writer)?;
        }

        log::info!("client({}): QUERYING STATE", timeline_id);
        doc.query(|tx| {
            tx.query_row("select value from counter", [], |row| {
                let value: Option<i32> = row.get(0)?;
                log::info!("client({}): counter value: {:?}", timeline_id, value);
                Ok(())
            })?;

            Ok::<_, anyhow::Error>(())
        })?;

        if let Some(lsn) = doc.storage_lsn() {
            // once the storage has reached (total_mutations+1) * num_clients
            // then we have reached the end
            log::info!("client({}): storage lsn: {}", timeline_id, lsn);
            if lsn >= ((total_mutations * num_clients) + 1) as Lsn {
                break;
            }
        }

        // small random sleep
        thread::sleep(std::time::Duration::from_millis(
            thread_rng().gen_range(0..100),
        ));
    }

    // final query, value should be total_mutations * num_clients
    doc.query(|tx| {
        tx.query_row_and_then("select value from counter", [], |row| {
            let value: Option<usize> = row.get(0)?;
            log::info!("client({}): FINAL counter value: {:?}", timeline_id, value);
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
        Ok::<_, anyhow::Error>(())
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
    let doc_id = JournalId::new256();

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
