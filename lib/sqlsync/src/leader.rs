use std::collections::HashMap;

use rusqlite::session::Session;

use crate::{Database, Mutator};

pub struct Leader<'a, M: Mutator> {
    mutator: M,
    db: Database,
    session: Option<Session<'a>>,

    // map from follower id to seq
    followers: HashMap<u64, u64>,
}

impl<'a, M: Mutator> Leader<'a, M> {
    pub fn new(mutator: M, db: Database) -> Self {
        Self {
            mutator,
            db,
            session: None,
            followers: HashMap::new(),
        }
    }


    pub fn apply(&mut self, mutation: M::Mutation) -> anyhow::Result<()> {
        self.db.run(|tx| self.mutator.apply(tx, &mutation))?;
        Ok(())
    }
}
