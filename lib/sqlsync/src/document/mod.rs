use std::fmt::Debug;

use anyhow::Result;
use rusqlite::Transaction;

use crate::{
    journal::{Journal, JournalPartial},
    lsn::{LsnRange, RequestedLsnRange},
    mutate::Mutator,
};

pub mod client;
pub mod server;

type DocumentId = i64;

pub trait Document<J: Journal, M: Mutator>: Debug + Sized {
    fn open(id: DocumentId, mutator: M) -> Result<Self>;

    fn sync_prepare(&self, req: RequestedLsnRange) -> Result<Option<JournalPartial<J::Iter<'_>>>>;
    fn sync_receive(&mut self, partial: JournalPartial<J::Iter<'_>>) -> Result<LsnRange>;
}

pub trait QueryableDocument {
    fn query<F>(&mut self, f: F) -> Result<()>
    where
        F: FnOnce(Transaction) -> Result<()>;
}

pub trait MutableDocument<M: Mutator> {
    fn mutate(&mut self, m: M::Mutation) -> Result<()>;
}

pub trait SteppableDocument {
    fn has_pending_work(&self) -> bool;
    fn step(&mut self) -> Result<()>;
}
