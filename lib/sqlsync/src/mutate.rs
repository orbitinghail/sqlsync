use rusqlite::Transaction;

use crate::{positioned_io::PositionedReader, Serializable};

pub trait Mutator: Clone {
    type Mutation: Serializable;

    fn apply(&self, tx: &mut Transaction, mutation: &Self::Mutation) -> anyhow::Result<()>;

    fn deserialize_mutation_from<R: PositionedReader>(
        &self,
        reader: R,
    ) -> anyhow::Result<Self::Mutation>;
}
