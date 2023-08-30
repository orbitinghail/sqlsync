use std::{cmp, io};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{lsn::LsnRange, positioned_io::PositionedReader, JournalId, Lsn};

// maximum number of frames we will send without receiving an acknowledgement
const MAX_OUTSTANDING_FRAMES: usize = 10;

#[derive(Clone, Serialize, Deserialize, Debug)]
pub enum ReplicationMsg {
    /// request the lsn range of the specified journal
    RangeRequest { id: JournalId },
    /// reply to a RangeRequest with the range of the specified journal
    Range { range: Option<LsnRange> },
    /// send one LSN frame from the specified journal
    Frame { id: JournalId, lsn: Lsn, len: u64 },
}

#[derive(Error, Debug)]
pub enum ReplicationError {
    #[error(transparent)]
    Io(#[from] io::Error),

    #[error(transparent)]
    Anyhow(#[from] anyhow::Error),

    #[error("replication protocol is uninitialized")]
    Uninitialized,

    #[error("unknown journal id: {0}")]
    UnknownJournal(JournalId),

    #[error(
        "replication must be contiguous, received lsn {received} but expected lsn in range {range}"
    )]
    NonContiguousLsn { received: Lsn, range: LsnRange },
}

// struct ReplicationState {
//     sent: LsnRange,
//     acknowledged: LsnRange,
// }

#[derive(Debug)]
pub enum ReplicationState {
    /// the replication protocol is waiting for the remote journal range
    Uninitialized,
    /// the replication protocol is fully initialized
    Initialized {
        /// the range of the destination journal
        destination_range: Option<LsnRange>,
        /// the range we have sent to the destination
        /// this range may be larger than the destination range if the
        /// destination is lagging on sending acknowledgements
        sent_range: Option<LsnRange>,
    },
}

#[derive(Debug)]
pub struct ReplicationProtocol {
    state: ReplicationState,
}

impl ReplicationProtocol {
    pub fn new() -> Self {
        Self {
            state: ReplicationState::Uninitialized,
        }
    }

    pub fn initialized(&self) -> bool {
        matches!(self.state, ReplicationState::Initialized { .. })
    }

    /// start replication, must be called on both sides of the connection
    pub fn start<D: ReplicationSource>(&self, doc: &D) -> ReplicationMsg {
        ReplicationMsg::RangeRequest {
            id: doc.source_id(),
        }
    }

    /// sync a frame from the source journal to the destination
    /// the protocol layer will need to send the replication msg
    /// followed by the contents of the reader to the destination
    pub fn sync<'a, D: ReplicationSource>(
        &mut self,
        doc: &'a D,
    ) -> Result<Option<(ReplicationMsg, D::Reader<'a>)>, ReplicationError> {
        let lsn = match self.state {
            ReplicationState::Uninitialized => {
                return Err(ReplicationError::Uninitialized);
            }
            ReplicationState::Initialized {
                destination_range: Some(dest),
                sent_range: Some(sent),
            } => {
                // we want to send the next lsn after sent_range, but only if the delta is less than MAX_OUTSTANDING_FRAMES
                if sent.last() - dest.last() < MAX_OUTSTANDING_FRAMES as u64 {
                    sent.last() + 1
                } else {
                    // nothing to sync
                    return Ok(None);
                }
            }
            ReplicationState::Initialized {
                destination_range: Some(dest),
                ..
            } => dest.last() + 1,
            ReplicationState::Initialized {
                destination_range: None,
                ..
            } => 0,
        };

        if let Some(data) = doc.read_lsn(lsn)? {
            Ok(Some((
                ReplicationMsg::Frame {
                    id: doc.source_id(),
                    lsn,
                    len: data.size()? as u64,
                },
                data,
            )))
        } else {
            Ok(None)
        }
    }

    /// handle a replication message from the remote side
    /// connection is needed to read additional bytes from the remote side
    /// this is used to synchronize frames without excessive buffering
    pub fn handle<D: ReplicationDestination>(
        &mut self,
        doc: &mut D,
        msg: ReplicationMsg,
        connection: &mut impl io::Read,
    ) -> Result<Option<ReplicationMsg>, ReplicationError> {
        match msg {
            ReplicationMsg::RangeRequest { id } => Ok(Some(ReplicationMsg::Range {
                range: doc.range(id)?,
            })),
            ReplicationMsg::Range { range } => {
                // NOTE TO CARL
                // I think the issue is that this range is being overwritten somehow
                // so possibly needs a union? or something like that
                self.state = ReplicationState::Initialized {
                    destination_range: range,
                    sent_range: None,
                };
                Ok(None)
            }
            ReplicationMsg::Frame { id, lsn, len } => match self.state {
                ReplicationState::Uninitialized => {
                    return Err(ReplicationError::Uninitialized);
                }
                ReplicationState::Initialized { .. } => {
                    let mut reader = LimitedReader {
                        limit: len,
                        inner: connection,
                    };
                    doc.write_lsn(id, lsn, &mut reader)?;
                    Ok(Some(ReplicationMsg::Range {
                        range: doc.range(id)?,
                    }))
                }
            },
        }
    }
}

pub trait ReplicationSource {
    type Reader<'a>: PositionedReader
    where
        Self: 'a;

    /// the id of the source journal
    fn source_id(&self) -> JournalId;

    /// read the given lsn from the source journal if it exists
    fn read_lsn<'a>(&'a self, lsn: Lsn) -> io::Result<Option<Self::Reader<'a>>>;
}

pub trait ReplicationDestination {
    fn range(&mut self, id: JournalId) -> Result<Option<LsnRange>, ReplicationError>;

    /// write the given lsn to the destination journal
    fn write_lsn<R>(
        &mut self,
        id: JournalId,
        lsn: Lsn,
        reader: &mut R,
    ) -> Result<(), ReplicationError>
    where
        R: io::Read;
}

/// LimitedReader is basically io::Take but over a mutable ref
struct LimitedReader<'a, R: io::Read> {
    limit: u64,
    inner: &'a mut R,
}

impl<'a, R: io::Read> io::Read for LimitedReader<'a, R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.limit == 0 {
            return Ok(0);
        }
        let max = cmp::min(buf.len() as u64, self.limit) as usize;
        let n = self.inner.read(&mut buf[..max])?;
        assert!(n as u64 <= self.limit, "number of read bytes exceeds limit");
        self.limit -= n as u64;
        Ok(n)
    }
}
