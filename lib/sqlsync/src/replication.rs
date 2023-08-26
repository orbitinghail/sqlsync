use std::io;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{JournalId, LsnRange, RequestedLsnRange, SyncError, Syncable, Cursor, positioned_io::PositionedReader};

#[derive(Clone, Serialize, Deserialize, Debug)]
pub enum ReplicationMsg {
    /// start replication from the given journal
    Init { id: JournalId },
    /// request a lsn range from the source journal
    Request { request_range: RequestedLsnRange },
    /// announce the start of series of frames from the source journal
    Start { range: LsnRange, frames: usize },
    /// send a frame from the source journal
    Frame { len: usize },
}

#[derive(Error, Debug)]
pub enum ReplicationError {
    #[error(transparent)]
    Sync(#[from] SyncError),

    #[error(transparent)]
    Io(#[from] io::Error),

    #[error("replication not initialized")]
    Uninitialized,

    #[error("replication already started for journal {0}")]
    AlreadyStarted(JournalId),
}

struct ReplicationMachine<T: Syncable> {
    doc: T,

    remote_id: Option<JournalId>,
    remote_req: Option<RequestedLsnRange>,
}

impl<T: Syncable> ReplicationMachine<T> {
    fn new(doc: T) -> Self {
        Self {
            doc,
            remote_id: None,
            remote_req: None,
        }
    }

    fn init(&self) -> ReplicationMsg {
        ReplicationMsg::Init {
            id: self.doc.source_id(),
        }
    }

    fn sync<S: ReplicationSocket>(&mut self, socket: S) -> Result<(), ReplicationError> {
        if let Some(req) = self.remote_req {
            if let Some(partial) = self.doc.sync_prepare(req)? {
                socket.send(ReplicationMsg::Start {
                    range: partial.range(),
                    frames: partial.len(),
                })?;
                let mut cursor = partial.into_read_partial().into_cursor();
                while cursor.advance()? {
                    let len = cursor.size()?;
                    socket.send(ReplicationMsg::Frame { len })?;
                    io::copy(&mut cursor, socket)?;
                }
            }
            Ok(())
        } else {
            Err(ReplicationError::Uninitialized)
        }
    }

    fn handle(&mut self, msg: ReplicationMsg) -> Result<Option<ReplicationMsg>, ReplicationError> {
        match msg {
            ReplicationMsg::Init { id } => {
                if self.remote_id.is_some() {
                    return Err(ReplicationError::AlreadyStarted(id));
                }
                self.remote_id = Some(id);
                let req = self.doc.sync_request(id)?;
                Ok(Some(ReplicationMsg::Request { request_range: req }))
            }
            ReplicationMsg::Request { request_range } => {
                self.remote_req = Some(request_range);
                Ok(None)
            }
            ReplicationMsg::Start { range, frames } => Ok(None),
            ReplicationMsg::Frame { len } => Ok(None),
        }
    }
}

trait ReplicationSocket {
    fn send(&self, msg: ReplicationMsg) -> Result<(), io::Error>;
    fn recv(&self) -> Result<ReplicationMsg, io::Error>;
}
