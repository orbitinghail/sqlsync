use std::convert;

use rusqlite::{params_from_iter, Connection, Row, ToSql};

use crate::{iter::has_sorted_intersection, PageIdx, StorageChange};

#[derive(Debug)]
enum State {
    // The query is pending refresh
    Dirty,

    // The query has been executed and the root pages have been fetched
    // The query is monitoring for changes to the root pages
    Monitoring { root_pages_sorted: Vec<PageIdx> },

    // The query failed last time it was run, we will only rerun the query if
    // the storage changes
    Error,
}

#[derive(Debug)]
pub struct ReactiveQuery<P: ToSql> {
    sql: String,
    explain_sql: String,
    params: Vec<P>,
    state: State,
}

impl<P: ToSql> ReactiveQuery<P> {
    pub fn new(sql: String, params: Vec<P>) -> Self {
        let explain_sql = format!("EXPLAIN {}", &sql);
        Self { sql, explain_sql, params, state: State::Dirty }
    }

    // handle_storage_change checks if the storage change affects this query
    // sets the state to dirty if it does
    // returns self.is_dirty()
    pub fn handle_storage_change(&mut self, change: &StorageChange) -> bool {
        match self.state {
            State::Dirty => {}
            State::Monitoring { root_pages_sorted: ref root_pages } => {
                match change {
                    StorageChange::Full => self.state = State::Dirty,
                    StorageChange::Tables {
                        root_pages_sorted: ref changed_root_pages,
                    } => {
                        if has_sorted_intersection(
                            root_pages,
                            changed_root_pages,
                        ) {
                            self.state = State::Dirty;
                        }
                    }
                }
            }
            State::Error => self.state = State::Dirty,
        }
        self.is_dirty()
    }

    #[inline]
    pub fn is_dirty(&self) -> bool {
        matches!(self.state, State::Dirty)
    }

    #[inline]
    pub fn mark_dirty(&mut self) {
        self.state = State::Dirty;
    }

    #[inline]
    pub fn mark_error(&mut self) {
        self.state = State::Error;
    }

    pub fn refresh<T, E, F>(
        &mut self,
        conn: &Connection,
        mut f: F,
    ) -> Result<(Vec<String>, Vec<T>), E>
    where
        E: convert::From<rusqlite::Error>,
        F: FnMut(&[String], &Row<'_>) -> Result<T, E>,
    {
        self.refresh_state(conn)?;

        let mut stmt = conn.prepare_cached(&self.sql)?;
        let columns: Vec<_> =
            stmt.column_names().iter().map(|&s| s.to_owned()).collect();
        let mut rows = stmt.query(params_from_iter(&self.params))?;
        let mut out = Vec::new();
        while let Some(row) = rows.next()? {
            let mapped = f(&columns, &row)?;
            out.push(mapped);
        }

        Ok((columns, out))
    }

    fn refresh_state(&mut self, conn: &Connection) -> rusqlite::Result<()> {
        let mut explain = conn.prepare_cached(&self.explain_sql)?;
        let mut rows = explain.query(params_from_iter(&self.params))?;

        let mut root_pages_sorted = Vec::new();
        while let Some(row) = rows.next()? {
            // explain rows have the schema:
            // addr, opcode, p1, p2, p3, p4, p5, comment
            // to find root pages, we need to find the OpenRead opcodes
            // and then look at the p2 column which contains the root page id
            let opcode: String = row.get(1)?;
            if opcode == "OpenRead" {
                let root_page: PageIdx = row.get(3)?;
                root_pages_sorted.push(root_page);
            }
        }

        root_pages_sorted.sort();
        root_pages_sorted.dedup();

        self.state = State::Monitoring { root_pages_sorted };
        Ok(())
    }
}
