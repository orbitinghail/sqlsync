mod utils;

use std::{io, any::Any};

use anyhow::anyhow;
use sqlsync::{
    local::LocalDocument, mutate::Mutator, positioned_io::PositionedReader, Cursor, Deserializable,
    Journal, JournalId, Lsn, MemoryJournal, Scannable, Serializable, Syncable, Transaction, ToSql,
};
use utils::{ConsoleLogger, WasmResult};
use wasm_bindgen::{convert::IntoWasmAbi, prelude::*};

static LOGGER: ConsoleLogger = ConsoleLogger;

#[wasm_bindgen(start)]
pub fn init() {
    utils::set_panic_hook();
    log::set_logger(&LOGGER).unwrap();
    log::set_max_level(log::LevelFilter::Info);
}

#[wasm_bindgen]
pub fn open(doc_id: JournalId, timeline_id: JournalId) -> WasmResult<SqlSyncDocument> {
    let storage = MemoryJournal::open(doc_id)?;
    let timeline = MemoryJournal::open(timeline_id)?;

    Ok(SqlSyncDocument {
        doc: LocalDocument::open(storage, timeline, MutatorImpl {})?,
    })
}

#[wasm_bindgen]
pub struct SqlSyncDocument {
    doc: LocalDocument<MemoryJournal, MutatorImpl>,
}

#[wasm_bindgen]
impl SqlSyncDocument {
    pub fn hello_world(&mut self) -> WasmResult<()> {
        log::info!("HELLO WORLD FROM WASM");
        Ok(self.doc.mutate(Mutation::Foo)?)
    }
}

#[wasm_bindgen]
extern "C" {
    type Mutation;

    fn mutation_from_bytes(data: Vec<u8>) -> Mutation;

    #[wasm_bindgen(method)]
    fn serialize(this: &Mutation) -> Vec<u8>;

    #[derive(Clone)]
    type MutatorHandle;

    #[wasm_bindgen(method, catch)]
    fn apply(
        this: &MutatorHandle,
        mutation: &Mutation,
        execute: &mut dyn FnMut(String, Vec<JsValue>) -> WasmResult<()>,
        // query: &mut dyn FnMut(String, Vec<JsValue>) -> WasmResult<()>,
    ) -> WasmResult<()>;
}

impl Mutator for MutatorHandle {
    type Mutation = Mutation;

    fn apply(&self, tx: &mut Transaction, mutation: &Self::Mutation) -> anyhow::Result<()> {
        self.apply(mutation, &mut |sql, params| {
            let params: Result<Vec<String>, _> = params.iter().map(|v| {
                if v.is_string() {
                    Ok(v.as_string().unwrap())
                } else {
                    Err(anyhow!("expected string"))
                }
            }).collect();
            Ok(tx.execute(&sql, params?)?)
            Ok(())
        });
        Ok(())
    }
}

impl Serializable for Mutation {
    fn serialize_into<W: io::Write>(&self, writer: &mut W) -> io::Result<()> {
        writer.write_all(&self.serialize())
    }
}

impl Deserializable for Mutation {
    fn deserialize_from<R: PositionedReader>(reader: R) -> io::Result<Self> {
        let data = reader.read_all()?;
        Ok(mutation_from_bytes(data))
    }
}

/// EVERYTHING PAST THIS LINE IS STUBBED

#[derive(Debug)]
struct OPFSJournal {}

impl Journal for OPFSJournal {
    fn open(id: sqlsync::JournalId) -> sqlsync::JournalResult<Self> {
        todo!()
    }

    fn id(&self) -> sqlsync::JournalId {
        todo!()
    }

    fn append(&mut self, obj: impl Serializable) -> sqlsync::JournalResult<()> {
        todo!()
    }

    fn drop_prefix(&mut self, up_to: Lsn) -> sqlsync::JournalResult<()> {
        todo!()
    }
}

struct OPFSCursor {}

impl Cursor for OPFSCursor {
    fn advance(&mut self) -> io::Result<bool> {
        todo!()
    }

    fn remaining(&self) -> usize {
        todo!()
    }
}

impl PositionedReader for OPFSCursor {
    fn read_at(&self, pos: usize, buf: &mut [u8]) -> io::Result<usize> {
        todo!()
    }

    fn size(&self) -> io::Result<usize> {
        todo!()
    }
}

impl Scannable for OPFSJournal {
    type Cursor<'a> = OPFSCursor
    where
        Self: 'a;

    fn scan<'a>(&'a self) -> Self::Cursor<'a> {
        todo!()
    }

    fn scan_rev<'a>(&'a self) -> Self::Cursor<'a> {
        todo!()
    }

    fn scan_range<'a>(&'a self, range: sqlsync::LsnRange) -> Self::Cursor<'a> {
        todo!()
    }
}

impl Syncable for OPFSJournal {
    type Cursor<'a> = OPFSCursor
    where
        Self: 'a;

    fn source_id(&self) -> sqlsync::JournalId {
        todo!()
    }

    fn sync_prepare<'a>(
        &'a mut self,
        req: sqlsync::RequestedLsnRange,
    ) -> sqlsync::SyncResult<Option<sqlsync::JournalPartial<Self::Cursor<'a>>>> {
        todo!()
    }

    fn sync_request(
        &mut self,
        id: sqlsync::JournalId,
    ) -> sqlsync::SyncResult<sqlsync::RequestedLsnRange> {
        todo!()
    }

    fn sync_receive<C>(
        &mut self,
        partial: sqlsync::JournalPartial<C>,
    ) -> sqlsync::SyncResult<sqlsync::LsnRange>
    where
        C: Cursor + io::Read,
    {
        todo!()
    }
}
