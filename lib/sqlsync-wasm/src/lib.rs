mod utils;

use std::io;

use js_sys::Reflect;
use sqlsync::{
    local::LocalDocument,
    mutate::Mutator,
    positioned_io::PositionedReader,
    sqlite::{params_from_iter, Transaction},
    Journal, JournalId, MemoryJournal, Serializable,
};
use utils::{ConsoleLogger, JsValueFromSql, JsValueToSql, WasmError, WasmResult};
use wasm_bindgen::prelude::*;

static LOGGER: ConsoleLogger = ConsoleLogger;

#[wasm_bindgen(typescript_custom_section)]
const TYPESCRIPT_INTERFACE: &'static str = r#"
type SqlValue = undefined | null | boolean | number | string;

type ExecuteFn = (sql: string, params: SqlValue[]) => number;

type QueryFn = (
  sql: string,
  params: SqlValue[]
) => { [key: string]: SqlValue }[];

interface IMutation {
    serialize(): Uint8Array;
}

interface IMutatorHandle<M extends IMutation> {
    apply(mutation: M, execute: ExecuteFn, query: QueryFn): void;
    deserializeMutation(data: Uint8Array): M;
}

interface SqlSyncDocument {
  query<T>(sql: string, params: SqlValue[]): T[];
  query(sql: string, params: SqlValue[]): object[];
}
"#;

#[wasm_bindgen(start)]
pub fn init() {
    utils::set_panic_hook();
    log::set_logger(&LOGGER).unwrap();
    log::set_max_level(log::LevelFilter::Info);
}

#[wasm_bindgen]
pub fn open(
    doc_id: JournalId,
    timeline_id: JournalId,
    mutator: MutatorHandle,
) -> WasmResult<SqlSyncDocument> {
    let storage = MemoryJournal::open(doc_id)?;
    let timeline = MemoryJournal::open(timeline_id)?;

    Ok(SqlSyncDocument {
        doc: LocalDocument::open(storage, timeline, mutator)?,
    })
}

#[wasm_bindgen]
pub struct SqlSyncDocument {
    doc: LocalDocument<MemoryJournal, MutatorHandle>,
}

#[wasm_bindgen]
impl SqlSyncDocument {
    pub fn mutate(&mut self, mutation: Mutation) -> WasmResult<()> {
        Ok(self.doc.mutate(mutation)?)
    }

    #[wasm_bindgen]
    pub fn query(&mut self, sql: String, params: Vec<JsValue>) -> WasmResult<Vec<js_sys::Object>> {
        Ok(self.doc.query(|tx| {
            let params = params_from_iter(params.iter().map(|v| JsValueToSql(v)));
            let mut stmt = tx.prepare(&sql)?;

            let column_names: Vec<_> = stmt.column_names().iter().map(|&s| s.to_owned()).collect();

            let out = stmt
                .query_and_then(params, move |row| {
                    let row_obj = js_sys::Object::new();
                    for (i, column_name) in column_names.iter().enumerate() {
                        Reflect::set(
                            &row_obj,
                            &column_name.into(),
                            &JsValueFromSql(row.get_ref(i)?).into(),
                        )?;
                    }
                    Ok::<_, WasmError>(row_obj)
                })?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| e.into_anyhow());
            out
        })?)
    }
}

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(typescript_type = "IMutation")]
    pub type Mutation;

    #[wasm_bindgen(method, catch)]
    fn serialize(this: &Mutation) -> WasmResult<Vec<u8>>;

    #[wasm_bindgen(typescript_type = "IMutatorHandle")]
    #[derive(Clone)]
    pub type MutatorHandle;

    #[wasm_bindgen(method, catch)]
    fn apply(
        this: &MutatorHandle,
        mutation: &Mutation,
        execute: &mut dyn FnMut(String, Vec<JsValue>) -> WasmResult<usize>,
        query: &mut dyn FnMut(String, Vec<JsValue>) -> WasmResult<Vec<js_sys::Object>>,
    ) -> WasmResult<()>;

    #[wasm_bindgen(method, catch)]
    fn deserializeMutation(this: &MutatorHandle, data: Vec<u8>) -> WasmResult<Mutation>;
}

impl Mutator for MutatorHandle {
    type Mutation = Mutation;

    fn apply(&self, tx: &mut Transaction, mutation: &Self::Mutation) -> anyhow::Result<()> {
        // create execute and query closure
        let execute = &mut |sql: String, params: Vec<JsValue>| {
            let params = params_from_iter(params.iter().map(|v| JsValueToSql(v)));
            Ok::<_, WasmError>(tx.execute(&sql, params)?)
        };

        let query = &mut |sql: String, params: Vec<JsValue>| {
            let params = params_from_iter(params.iter().map(|v| JsValueToSql(v)));
            let mut stmt = tx.prepare(&sql)?;

            let column_names: Vec<_> = stmt.column_names().iter().map(|&s| s.to_owned()).collect();

            let rows = stmt
                .query_and_then(params, move |row| {
                    let row_obj = js_sys::Object::new();
                    for (i, column_name) in column_names.iter().enumerate() {
                        Reflect::set(
                            &row_obj,
                            &column_name.into(),
                            &JsValueFromSql(row.get_ref(i)?).into(),
                        )?;
                    }
                    Ok::<_, WasmError>(row_obj)
                })?
                .collect::<Result<Vec<_>, _>>()?;

            Ok::<_, WasmError>(rows)
        };

        self.apply(mutation, execute, query)
            .map_err(|err| err.into_anyhow())
    }

    fn deserialize_mutation_from<R: PositionedReader>(
        &self,
        reader: R,
    ) -> anyhow::Result<Self::Mutation> {
        let data = reader.read_all()?;
        self.deserializeMutation(data)
            .map_err(|err| err.into_anyhow())
    }
}

impl Serializable for Mutation {
    fn serialize_into<W: io::Write>(&self, writer: &mut W) -> io::Result<()> {
        writer.write_all(&self.serialize()?)
    }
}

// EVERYTHING PAST THIS LINE IS STUBBED

// #[derive(Debug)]
// struct OPFSJournal {}

// impl Journal for OPFSJournal {
//     fn open(id: sqlsync::JournalId) -> sqlsync::JournalResult<Self> {
//         todo!()
//     }

//     fn id(&self) -> sqlsync::JournalId {
//         todo!()
//     }

//     fn append(&mut self, obj: impl Serializable) -> sqlsync::JournalResult<()> {
//         todo!()
//     }

//     fn drop_prefix(&mut self, up_to: Lsn) -> sqlsync::JournalResult<()> {
//         todo!()
//     }
// }

// struct OPFSCursor {}

// impl Cursor for OPFSCursor {
//     fn advance(&mut self) -> io::Result<bool> {
//         todo!()
//     }

//     fn remaining(&self) -> usize {
//         todo!()
//     }
// }

// impl PositionedReader for OPFSCursor {
//     fn read_at(&self, pos: usize, buf: &mut [u8]) -> io::Result<usize> {
//         todo!()
//     }

//     fn size(&self) -> io::Result<usize> {
//         todo!()
//     }
// }

// impl Scannable for OPFSJournal {
//     type Cursor<'a> = OPFSCursor
//     where
//         Self: 'a;

//     fn scan<'a>(&'a self) -> Self::Cursor<'a> {
//         todo!()
//     }

//     fn scan_rev<'a>(&'a self) -> Self::Cursor<'a> {
//         todo!()
//     }

//     fn scan_range<'a>(&'a self, range: sqlsync::LsnRange) -> Self::Cursor<'a> {
//         todo!()
//     }
// }

// impl Syncable for OPFSJournal {
//     type Cursor<'a> = OPFSCursor
//     where
//         Self: 'a;

//     fn source_id(&self) -> sqlsync::JournalId {
//         todo!()
//     }

//     fn sync_prepare<'a>(
//         &'a mut self,
//         req: sqlsync::RequestedLsnRange,
//     ) -> sqlsync::SyncResult<Option<sqlsync::JournalPartial<Self::Cursor<'a>>>> {
//         todo!()
//     }

//     fn sync_request(
//         &mut self,
//         id: sqlsync::JournalId,
//     ) -> sqlsync::SyncResult<sqlsync::RequestedLsnRange> {
//         todo!()
//     }

//     fn sync_receive<C>(
//         &mut self,
//         partial: sqlsync::JournalPartial<C>,
//     ) -> sqlsync::SyncResult<sqlsync::LsnRange>
//     where
//         C: Cursor + io::Read,
//     {
//         todo!()
//     }
// }
