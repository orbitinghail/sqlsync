use rusqlite::Transaction;

use crate::journal::{Deserializable, Serializable};

pub trait Mutator: Clone {
    type Mutation: Serializable + Deserializable;
    fn apply(&self, tx: &mut Transaction, mutation: &Self::Mutation) -> anyhow::Result<()>;
}
