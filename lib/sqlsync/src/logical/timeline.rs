use std::ops::DerefMut;

use rusqlite::Connection;

use crate::{db::run_in_tx, Mutator};

#[derive(Clone)]
struct Entry<M: Clone> {
    seq: u64,
    mutation: M,
}

struct Batch<M> {
    timeline_id: u64,
    last_seq: u64,
    mutations: Vec<M>,
}

struct Timeline<M: Mutator> {
    id: u64,
    next_seq: u64,
    mutator: M,
    entries: Vec<Entry<M::Mutation>>,
}

impl<M: Mutator> Timeline<M> {
    pub fn new(id: u64, mutator: M) -> Self {
        Self {
            id,
            next_seq: 0,
            mutator,
            entries: Vec::new(),
        }
    }

    pub fn run(&mut self, sqlite: &mut Connection, mutation: M::Mutation) -> anyhow::Result<()> {
        run_in_tx(sqlite, |tx| {
            self.mutator.apply(tx, &mutation)?;
            self.entries.push(Entry {
                seq: self.next_seq,
                mutation,
            });
            self.next_seq += 1;
            Ok(())
        })
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn read_batch(&self, start_seq: u64, max_size: usize) -> Option<Batch<M::Mutation>> {
        if self.entries.is_empty() {
            return None;
        }

        let entries: Vec<Entry<M::Mutation>> = self
            .entries
            .iter()
            .skip_while(|e| e.seq < start_seq)
            .take(max_size)
            .map(|e| e.clone())
            .collect();

        let last_seq = entries.last().map(|e| e.seq)?;
        let mutations = entries.into_iter().map(|e| e.mutation).collect();

        Some(Batch {
            timeline_id: self.id,
            last_seq,
            mutations,
        })
    }

    pub fn rebase(&self, sqlite: &mut Connection) -> anyhow::Result<()> {
        todo!(
            "
            * read seq from timelines table
            * remove mutations <= seq
            * reapply remaining mutations
            "
        )
    }
}
