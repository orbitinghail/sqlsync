use rusqlite::Transaction;

pub trait Mutator {
    type Mutation: Clone;
    fn apply(&self, tx: &mut Transaction, mutation: &Self::Mutation) -> anyhow::Result<()>;
}
