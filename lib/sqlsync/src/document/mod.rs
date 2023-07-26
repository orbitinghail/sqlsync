use std::fmt::Debug;

use anyhow::Result;
use rusqlite::Transaction;

use crate::{
    journal::{Journal, JournalIterator},
    lsn::{LsnRange, RequestedLsnRange},
    mutate::Mutator,
};

pub mod client;
pub mod server;

pub type DocumentId = i64;

pub trait Document: Debug + Sized {
    type J: Journal;
    type M: Mutator;

    fn open(id: DocumentId, mutator: Self::M) -> Result<Self>;

    fn sync_prepare(&self, req: RequestedLsnRange) -> Result<Option<<Self::J as Journal>::Iter>>;
    fn sync_receive(&mut self, partial: impl JournalIterator) -> Result<LsnRange>;
}

pub trait QueryableDocument {
    fn query<F>(&mut self, f: F) -> Result<()>
    where
        F: FnOnce(Transaction) -> Result<()>;
}

pub trait MutableDocument {
    type Mutation;

    fn mutate(&mut self, m: Self::Mutation) -> Result<()>;
}

pub trait SteppableDocument {
    fn has_pending_work(&self) -> bool;
    fn step(&mut self) -> Result<()>;
}
