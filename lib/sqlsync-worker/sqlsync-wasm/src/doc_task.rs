use anyhow::anyhow;
use futures::{channel::mpsc, select, FutureExt, StreamExt};
use rand::thread_rng;
use sqlsync::{
    local::LocalDocument, sqlite::params_from_iter, JournalId, MemoryJournal,
    Reducer,
};

use crate::{
    api::{
        DocEvent, DocReply, DocRequest, HostToWorkerMsg, PortRouter,
        WorkerToHostMsg,
    },
    net::{ConnectionTask, CoordinatorClient},
    reactive::ReactiveQueries,
    signal::{SignalEmitter, SignalRouter},
    sql::SqlValue,
    utils::{WasmError, WasmResult},
};

#[derive(Clone, Copy, Hash, PartialEq, Eq)]
enum Signal {
    StorageChanged,
    TimelineChanged,
    CanRebase,
    HasDirtyQueries,
    ConnectionStateChanged,
}

pub struct DocTask {
    doc: LocalDocument<MemoryJournal, SignalEmitter<Signal>>,
    inbox: mpsc::UnboundedReceiver<HostToWorkerMsg>,
    signals: SignalRouter<Signal>,
    ports: PortRouter,
    queries: ReactiveQueries<SignalEmitter<Signal>>,
    coordinator_client: CoordinatorClient<SignalEmitter<Signal>>,
}

impl DocTask {
    pub fn new(
        doc_id: JournalId,
        doc_url: Option<String>,
        reducer: Reducer,
        inbox: mpsc::UnboundedReceiver<HostToWorkerMsg>,
        ports: PortRouter,
    ) -> WasmResult<Self> {
        // TODO: use persisted timeline id when we start persisting the journal to OPFS
        let timeline_id = JournalId::new128(&mut thread_rng());

        let signals = SignalRouter::new();

        let storage = MemoryJournal::open(doc_id)?;
        let timeline = MemoryJournal::open(timeline_id)?;
        let doc = LocalDocument::open(
            storage,
            timeline,
            reducer,
            signals.emitter(Signal::StorageChanged),
            signals.emitter(Signal::TimelineChanged),
            signals.emitter(Signal::CanRebase),
        )?;

        let queries =
            ReactiveQueries::new(signals.emitter(Signal::HasDirtyQueries));
        let coordinator_client = CoordinatorClient::new(
            doc_url,
            signals.emitter(Signal::ConnectionStateChanged),
        );

        Ok(Self { doc, inbox, signals, ports, queries, coordinator_client })
    }

    pub async fn into_task(mut self) {
        // NOTE TO CODE REVIEWERS:
        // `select!` is full of foot guns (see: [1] and [2])
        // It's only safe if each branch follows these rules:
        //  - if ready, return value without awaiting
        //  - if not ready, await precisely once and then return value
        //
        // Critically: if it's possible to await twice during the execution of a
        // single future handled by select! - then it's possible for the future
        // to be dropped in an intermediate state.
        //
        // [1]: https://tomaka.medium.com/a-look-back-at-asynchronous-rust-d54d63934a1c
        // [2]: https://blog.yoshuawuyts.com/futures-concurrency-3/

        loop {
            select! {
                signals = self.signals.listen().fuse() => {
                    self.handle_signals(signals).await;
                },
                task = self.coordinator_client.poll().fuse() => {
                    self.coordinator_client.handle(&mut self.doc, task).await;
                },
                msg = self.inbox.select_next_some() => {
                    self.handle_message(msg).await;
                },
            }
        }
    }

    async fn handle_signals(&mut self, signals: Vec<Signal>) {
        for signal in signals {
            match signal {
                Signal::ConnectionStateChanged => {
                    self.handle_connection_state_changed()
                }
                Signal::TimelineChanged => self.handle_timeline_changed().await,
                Signal::HasDirtyQueries => self.handle_dirty_queries(),

                Signal::StorageChanged => {
                    if let Err(e) = self.handle_storage_changed() {
                        panic!("failed to handle storage changes, the database is probably corrupted: {:?}", e);
                    }
                }

                Signal::CanRebase => {
                    if let Err(e) = self.doc.rebase() {
                        panic!("failed to rebase the document; this may mean that a mutation is failing to apply: {:?}", e);
                    }
                }
            }
        }
    }

    fn handle_connection_state_changed(&mut self) {
        let _ = self.ports.send_all(WorkerToHostMsg::Event {
            doc_id: self.doc.doc_id(),
            evt: DocEvent::ConnectionStatus {
                status: self.coordinator_client.status(),
            },
        });
    }

    fn handle_storage_changed(&mut self) -> anyhow::Result<()> {
        let changes = self.doc.storage_changes()?;
        log::debug!("storage changed: {:?}", changes);
        self.queries.handle_storage_change(&changes);
        Ok(())
    }

    async fn handle_timeline_changed(&mut self) {
        self.coordinator_client
            .handle(&mut self.doc, ConnectionTask::Sync)
            .await;
    }

    fn handle_dirty_queries(&mut self) {
        if let Some(query) = self.queries.next_dirty_query() {
            let result =
                query.refresh(self.doc.sqlite_readonly(), |columns, row| {
                    let mut out = Vec::with_capacity(columns.len());
                    for i in 0..columns.len() {
                        let val: SqlValue = row.get_ref(i)?.into();
                        out.push(val);
                    }
                    Ok::<_, WasmError>(out)
                });

            let msg = match result {
                Ok((columns, rows)) => WorkerToHostMsg::Event {
                    doc_id: self.doc.doc_id(),
                    evt: DocEvent::SubscriptionChanged {
                        key: query.query_key().clone(),
                        columns,
                        rows,
                    },
                },
                Err(err) => {
                    query.mark_error();
                    WorkerToHostMsg::Event {
                        doc_id: self.doc.doc_id(),
                        evt: DocEvent::SubscriptionErr {
                            key: query.query_key().clone(),
                            err: err.to_string(),
                        },
                    }
                }
            };

            if let Err(err) = self.ports.send_many(query.ports().clone(), msg) {
                self.queries.unsubscribe_all(&err.missing_ports());
            }
        }
    }

    async fn handle_message(&mut self, msg: HostToWorkerMsg) {
        match self.process_request(&msg).await {
            Ok(reply) => {
                log::info!("doc task reply: {:?}", reply);
                let _ = self.ports.send_one(msg.port_id, msg.reply(reply));
            }
            Err(err) => {
                log::info!("doc task error: {:?}", err);
                let _ = self.ports.send_one(msg.port_id, msg.reply_err(err));
            }
        }
    }

    async fn process_request(
        &mut self,
        msg: &HostToWorkerMsg,
    ) -> WasmResult<DocReply> {
        log::info!("DocTask::process_request: {:?}", msg.req);
        match &msg.req {
            DocRequest::Open { .. } => {
                Err(WasmError(anyhow!("doc is already open")))
            }

            DocRequest::Query { sql, params } => self.doc.query(|conn| {
                let params = params_from_iter(params.iter());
                let mut stmt = conn.prepare(sql)?;

                let columns: Vec<_> =
                    stmt.column_names().iter().map(|&s| s.to_owned()).collect();

                let rows = stmt
                    .query_and_then(params, |row| {
                        let mut out = Vec::with_capacity(columns.len());
                        for i in 0..columns.len() {
                            let val: SqlValue = row.get_ref(i)?.into();
                            out.push(val);
                        }
                        Ok::<_, WasmError>(out)
                    })?
                    .collect::<Result<Vec<_>, _>>()?;

                Ok::<_, WasmError>(DocReply::RecordSet { columns, rows })
            }),

            DocRequest::QuerySubscribe { key, sql, params } => {
                self.queries
                    .subscribe(msg.port_id, key, sql, params.to_vec());
                Ok(DocReply::Ack)
            }

            DocRequest::QueryUnsubscribe { key } => {
                self.queries.unsubscribe(msg.port_id, &key);
                Ok(DocReply::Ack)
            }

            DocRequest::Mutate { mutation } => {
                self.doc.mutate(&mutation.to_vec())?;
                Ok(DocReply::Ack)
            }

            DocRequest::RefreshConnectionStatus => {
                let _ = self.ports.send_one(
                    msg.port_id,
                    WorkerToHostMsg::Event {
                        doc_id: self.doc.doc_id(),
                        evt: DocEvent::ConnectionStatus {
                            status: self.coordinator_client.status(),
                        },
                    },
                );

                Ok(DocReply::Ack)
            }

            DocRequest::SetConnectionEnabled { enabled } => {
                let task = match enabled {
                    false => ConnectionTask::Disable,
                    true => {
                        if self.coordinator_client.can_enable() {
                            ConnectionTask::Connect
                        } else {
                            return Err(WasmError(anyhow!(
                                "cannot enable connection without coordinator url"
                            )));
                        }
                    }
                };
                self.coordinator_client.handle(&mut self.doc, task).await;

                Ok(DocReply::Ack)
            }
        }
    }
}
