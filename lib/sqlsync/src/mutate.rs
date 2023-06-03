use log::debug;
use rusqlite::Transaction;

use crate::Database;

pub trait Mutator {
    type Mutation;
    fn apply(&self, tx: &mut Transaction, mutation: &Self::Mutation) -> anyhow::Result<()>;
}

pub struct Entry<M: Mutator> {
    seq: u64,
    mutation: M::Mutation,
}

pub struct Follower<M: Mutator> {
    mutator: M,
    db: Database,
    next_seq: u64,
    timeline: Vec<Entry<M>>,
}

impl<M: Mutator> Follower<M> {
    pub fn new(mutator: M, db: Database) -> Self {
        Self {
            mutator,
            db,
            next_seq: 0,
            timeline: Vec::new(),
        }
    }

    pub fn seq(&self) -> u64 {
        self.timeline.last().map(|entry| entry.seq).unwrap_or(0)
    }

    pub fn rebase(&mut self, seq: u64) -> anyhow::Result<()> {
        debug!("rebase to seq {} (current seq: {})", seq, self.seq());

        // rollback the db
        self.db.rollback();

        // apply server changes
        // TODO: this needs to actually consume CDC from server
        // for now, we just play our timeline up to and including seq
        let mut last_seq = 0;
        for entry in &self.timeline {
            if entry.seq > seq {
                break;
            }
            debug!("replaying seq {}", entry.seq);
            last_seq = entry.seq;
            self.db.run(|tx| self.mutator.apply(tx, &entry.mutation))?;
        }

        // commit server changes
        debug!("committing seq {}", last_seq);
        self.db.commit();

        // remove all elements from the timeline up to and including seq
        self.timeline.retain(|entry| entry.seq > seq);

        // replay all remaining mutations
        for entry in &self.timeline {
            debug!("replaying seq {}", entry.seq);
            self.db.run(|tx| self.mutator.apply(tx, &entry.mutation))?;
        }

        Ok(())
    }

    pub fn apply(&mut self, mutation: M::Mutation) -> anyhow::Result<()> {
        // apply transaction locally
        self.db.run(|tx| self.mutator.apply(tx, &mutation))?;
        // record the mutation
        self.timeline.push(Entry {
            seq: self.next_seq,
            mutation,
        });
        // increment our sequence number
        self.next_seq += 1;
        Ok(())
    }

    pub fn query<F>(&mut self, f: F) -> anyhow::Result<()>
    where
        F: FnOnce(Transaction) -> anyhow::Result<()>,
    {
        self.db.query(f)
    }
}
