use std::{cmp, io};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{lsn::LsnRange, positioned_io::PositionedReader, JournalId, Lsn};

// maximum number of frames we will send without receiving an acknowledgement
// note: this does not affect durability, as we keep don't truncate the source journal until rebase
const MAX_OUTSTANDING_FRAMES: usize = 100;

#[derive(Clone, Serialize, Deserialize, Debug)]
pub enum ReplicationMsg {
    /// request the lsn range of the specified journal
    RangeRequest {
        id: JournalId,
        source_range: LsnRange,
    },
    /// reply to a RangeRequest with the range of the specified journal
    Range { range: LsnRange },
    /// send one LSN frame from the specified journal
    Frame { id: JournalId, lsn: Lsn, len: u64 },
}

#[derive(Error, Debug)]
pub enum ReplicationError {
    #[error(transparent)]
    Io(#[from] io::Error),

    // #[error("replication protocol is uninitialized")]
    // Uninitialized,
    #[error("unknown journal id: {0}")]
    UnknownJournal(JournalId),

    #[error(
        "replication must be contiguous, received lsn {received} but expected lsn in range {range}"
    )]
    NonContiguousLsn { received: Lsn, range: LsnRange },
}

#[derive(Debug)]
pub struct ReplicationProtocol {
    // outstanding lsn frames sent to the destination but awaiting acknowledgement
    // this is an Option because we need the to initialize it from the initial RangeRequest
    outstanding_range: Option<LsnRange>,
}

impl ReplicationProtocol {
    pub fn new() -> Self {
        Self { outstanding_range: None }
    }

    /// start replication, must be called on both sides of the connection
    pub fn start<D: ReplicationSource>(&self, doc: &D) -> ReplicationMsg {
        // before we can start sending frames to the destination, we need to know
        // what frames the destination already has
        ReplicationMsg::RangeRequest {
            id: doc.source_id(),
            source_range: doc.source_range(),
        }
    }

    /// initialized returns true if we have received a response to our initial range request
    /// and thus can start replicating data
    pub fn initialized(&self) -> bool {
        self.outstanding_range.is_some()
    }

    /// sync a frame from the source journal to the destination
    /// the protocol layer will need to send the replication msg
    /// followed by the contents of the reader to the destination
    pub fn sync<'a, D: ReplicationSource>(
        &mut self,
        doc: &'a D,
    ) -> Result<Option<(ReplicationMsg, D::Reader<'a>)>, ReplicationError> {
        if let Some(outstanding_range) = self.outstanding_range {
            if outstanding_range.len() >= MAX_OUTSTANDING_FRAMES {
                // we have too many outstanding frames, so we can't send any more
                return Ok(None);
            }

            let lsn = outstanding_range.next();
            if let Some(data) = doc.read_lsn(lsn)? {
                // update outstanding
                self.outstanding_range = Some(outstanding_range.append(lsn));

                // send frame
                return Ok(Some((
                    ReplicationMsg::Frame {
                        id: doc.source_id(),
                        lsn,
                        len: data.size()? as u64,
                    },
                    data,
                )));
            }
        }

        Ok(None)
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
            ReplicationMsg::RangeRequest { id, source_range } => {
                let mut range = doc.range(id)?;

                // if our range is empty, then we should reset to the remote's source range
                // this is to handle timeline truncation until we have a more reliable mechanism
                if range.is_empty() {
                    range = LsnRange::empty_preceeding(&source_range);
                }

                Ok(Some(ReplicationMsg::Range { range }))
            }
            ReplicationMsg::Range { range } => {
                self.outstanding_range = self.outstanding_range.map_or_else(
                    // first range response, initialize outstanding_range from destination range
                    || Some(LsnRange::empty_following(&range)),
                    // subsequent range response, update outstanding range
                    |outstanding_range| {
                        let next = range.next();
                        assert!(
                            next > 0,
                            "subsequent range responses should never be empty"
                        );
                        Some(outstanding_range.trim_prefix(next - 1))
                    },
                );
                Ok(None)
            }
            ReplicationMsg::Frame { id, lsn, len } => {
                let mut reader =
                    LimitedReader { limit: len, inner: connection };
                doc.write_lsn(id, lsn, &mut reader)?;
                Ok(Some(ReplicationMsg::Range { range: doc.range(id)? }))
            }
        }
    }
}

pub trait ReplicationSource {
    type Reader<'a>: PositionedReader
    where
        Self: 'a;

    /// the id of the source journal
    fn source_id(&self) -> JournalId;

    /// the range of the source journal
    fn source_range(&self) -> LsnRange;

    /// read the given lsn from the source journal if it exists
    fn read_lsn<'a>(&'a self, lsn: Lsn)
        -> io::Result<Option<Self::Reader<'a>>>;
}

pub trait ReplicationDestination {
    fn range(&mut self, id: JournalId) -> Result<LsnRange, ReplicationError>;

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
