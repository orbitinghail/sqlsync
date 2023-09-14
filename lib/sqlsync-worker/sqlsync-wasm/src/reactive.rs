use std::{
    collections::BTreeMap,
    ops::{Deref, DerefMut},
};

use sqlsync::{local::Signal, ReactiveQuery, StorageChange};

use crate::{api::PortId, sql::SqlValue};

pub type QueryKey = String;

#[derive(Debug)]
pub struct QueryTracker {
    query_key: QueryKey,
    query: ReactiveQuery<SqlValue>,
    ports: Vec<PortId>,
}

impl QueryTracker {
    pub fn query_key(&self) -> &QueryKey {
        &self.query_key
    }

    pub fn ports(&self) -> &Vec<PortId> {
        &self.ports
    }
}

impl Deref for QueryTracker {
    type Target = ReactiveQuery<SqlValue>;

    fn deref(&self) -> &Self::Target {
        &self.query
    }
}

impl DerefMut for QueryTracker {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.query
    }
}

pub struct ReactiveQueries<S: Signal> {
    queries: BTreeMap<QueryKey, QueryTracker>,
    has_dirty_queries: S,
}

impl<S: Signal> ReactiveQueries<S> {
    pub fn new(has_dirty_queries: S) -> Self {
        Self { queries: BTreeMap::new(), has_dirty_queries }
    }

    pub fn handle_storage_change(&mut self, change: &StorageChange) {
        let mut dirty = false;
        for tracker in self.queries.values_mut() {
            let d = tracker.query.handle_storage_change(change);
            dirty = dirty || d;
        }
        if dirty {
            self.has_dirty_queries.emit();
        }
    }

    pub fn subscribe(
        &mut self,
        port: PortId,
        key: &QueryKey,
        sql: &str,
        params: Vec<SqlValue>,
    ) {
        let tracker =
            self.queries
                .entry(key.clone())
                .or_insert_with(|| QueryTracker {
                    query_key: key.clone(),
                    query: ReactiveQuery::new(sql.to_owned(), params),
                    ports: Vec::new(),
                });

        // store the port, if it's not already subscribed
        if !tracker.ports.contains(&port) {
            tracker.ports.push(port);
        }

        // for now, we always mark the query as dirty when we subscribe
        // TODO: only refresh the query for the new subscriber
        tracker.query.mark_dirty();
        self.has_dirty_queries.emit();
    }

    pub fn unsubscribe(&mut self, port: PortId, query_key: &QueryKey) {
        if let Some(tracker) = self.queries.get_mut(query_key) {
            tracker.ports.retain(|p| p != &port);
            if tracker.ports.is_empty() {
                self.queries.remove(query_key);
            }
        }
    }

    pub fn unsubscribe_all(&mut self, ports: &Vec<PortId>) {
        for tracker in self.queries.values_mut() {
            tracker.ports.retain(|p| !ports.contains(p));
        }
        self.queries.retain(|_, tracker| !tracker.ports.is_empty());
    }

    /// next_dirty_query returns the first dirty query, and sets
    /// self.has_dirty_queries if there are more
    pub fn next_dirty_query(&mut self) -> Option<&mut QueryTracker> {
        let mut iter = self
            .queries
            .values_mut()
            .filter(|tracker| tracker.query.is_dirty());
        let first = iter.next();
        let has_more = iter.next().is_some();
        if has_more {
            self.has_dirty_queries.emit();
        }
        first
    }
}
