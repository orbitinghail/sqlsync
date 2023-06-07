mod local;
mod physical;
mod vfs;

pub use local::Local;

pub use rusqlite::{named_params, OptionalExtension, Transaction};

pub trait Mutator {
    type Mutation;
    fn apply(&self, tx: &mut Transaction, mutation: &Self::Mutation) -> anyhow::Result<()>;
}
