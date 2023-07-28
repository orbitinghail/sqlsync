use std::fmt::{Debug, Formatter};
use std::io;

use crate::lsn::{Lsn, LsnRange, RequestedLsnRange, SatisfyError};
use crate::positioned_io::PositionedReader;
use crate::{JournalError, Serializable};

use super::sync::JournalPartial;
use super::{
    Cursor, Journal, JournalId, JournalResult, Scannable, SyncError, SyncResult, Syncable,
};

// TODO: this should be smarter, syncing 5 journal frames is not optimal in many cases
const DEFAULT_REQUEST_LEN: usize = 5;

pub enum MemoryJournal {
    Empty {
        id: JournalId,
        nextlsn: Lsn,
    },
    NonEmpty {
        id: JournalId,
        range: LsnRange,
        data: Vec<Vec<u8>>,
    },
}

impl Debug for MemoryJournal {
    fn fmt(&self, f: &mut Formatter) -> std::fmt::Result {
        match self {
            MemoryJournal::Empty { id, nextlsn, .. } => f
                .debug_tuple("MemoryJournal::Empty")
                .field(id)
                .field(nextlsn)
                .finish(),
            MemoryJournal::NonEmpty { id, range, .. } => f
                .debug_tuple("MemoryJournal::NonEmpty")
                .field(id)
                .field(range)
                .finish(),
        }
    }
}

impl Journal for MemoryJournal {
    fn open(id: JournalId) -> JournalResult<Self> {
        Ok(MemoryJournal::Empty { id, nextlsn: 0 })
    }

    fn id(&self) -> JournalId {
        match self {
            Self::Empty { id, .. } => *id,
            Self::NonEmpty { id, .. } => *id,
        }
    }

    fn append(&mut self, obj: impl Serializable) -> JournalResult<()> {
        // serialize the entry
        let mut entry: Vec<u8> = Vec::new();
        obj.serialize_into(&mut entry)
            .map_err(|err| JournalError::SerializationError(err))?;

        // update the journal
        match self {
            MemoryJournal::Empty { id, nextlsn } => {
                *self = MemoryJournal::NonEmpty {
                    id: *id,
                    range: LsnRange::new(*nextlsn, *nextlsn),
                    data: vec![entry],
                };
            }
            MemoryJournal::NonEmpty { range, data, .. } => {
                data.push(entry);
                *range = range.extend_by(1);
            }
        }
        Ok(())
    }

    fn drop_prefix(&mut self, up_to: Lsn) -> JournalResult<()> {
        match self {
            MemoryJournal::Empty { .. } => Ok(()),
            MemoryJournal::NonEmpty { id, range, data } => {
                if let Some(remaining_range) = range.trim_prefix(up_to) {
                    let offsets = range.intersection_offsets(&remaining_range);
                    *data = data[offsets].to_vec();
                    *range = remaining_range;
                } else {
                    *self = MemoryJournal::Empty {
                        id: *id,
                        nextlsn: range.last() + 1,
                    };
                }
                Ok(())
            }
        }
    }
}

pub struct MemoryScanCursor<'a> {
    slice: &'a [Vec<u8>],
    started: bool,
    rev: bool,
}

impl<'a> MemoryScanCursor<'a> {
    fn new(slice: &'a [Vec<u8>]) -> Self {
        Self {
            slice,
            started: false,
            rev: false,
        }
    }

    fn empty() -> Self {
        Self {
            slice: &[],
            started: false,
            rev: false,
        }
    }

    fn reverse(mut self) -> Self {
        self.rev = !self.rev;
        self
    }

    fn get(&self) -> Option<&Vec<u8>> {
        if self.started {
            if self.rev {
                self.slice.last()
            } else {
                self.slice.first()
            }
        } else {
            None
        }
    }
}

impl<'a> Cursor for MemoryScanCursor<'a> {
    fn advance(&mut self) -> io::Result<bool> {
        if !self.started {
            self.started = true;
        } else {
            if self.rev {
                // remove the last element
                self.slice = &self.slice[..self.slice.len() - 1];
            } else {
                // remove the first element
                self.slice = &self.slice[1..];
            }
        }
        Ok(!self.slice.is_empty())
    }

    fn remaining(&self) -> usize {
        self.slice.len()
    }
}

impl<'a> PositionedReader for MemoryScanCursor<'a> {
    fn read_at(&self, pos: usize, buf: &mut [u8]) -> io::Result<usize> {
        match self.get() {
            None => Ok(0),
            Some(data) => data.read_at(pos, buf),
        }
    }

    fn size(&self) -> io::Result<usize> {
        match self.get() {
            None => Ok(0),
            Some(data) => Ok(data.len()),
        }
    }
}

impl Scannable for MemoryJournal {
    type Cursor<'a> = MemoryScanCursor<'a>
    where
        Self: 'a;

    fn scan<'a>(&'a self) -> Self::Cursor<'a> {
        match self {
            Self::Empty { .. } => MemoryScanCursor::empty(),
            Self::NonEmpty { data, .. } => MemoryScanCursor::new(data),
        }
    }

    fn scan_rev<'a>(&'a self) -> Self::Cursor<'a> {
        match self {
            Self::Empty { .. } => MemoryScanCursor::empty(),
            Self::NonEmpty { data, .. } => MemoryScanCursor::new(data).reverse(),
        }
    }

    fn scan_range<'a>(&'a self, range: LsnRange) -> Self::Cursor<'a> {
        match self {
            Self::Empty { .. } => MemoryScanCursor::empty(),
            Self::NonEmpty {
                range: journal_range,
                data,
                ..
            } => journal_range.intersect(&range).map_or_else(
                || MemoryScanCursor::empty(),
                |intersection_range| {
                    let offsets = journal_range.intersection_offsets(&intersection_range);
                    MemoryScanCursor::new(&data[offsets])
                },
            ),
        }
    }
}

impl Syncable for MemoryJournal {
    type Cursor<'a> = MemoryScanCursor<'a>
    where
        Self: 'a;

    fn source_id(&self) -> JournalId {
        self.id()
    }

    fn sync_prepare<'a>(
        &'a mut self,
        req: RequestedLsnRange,
    ) -> SyncResult<Option<JournalPartial<Self::Cursor<'a>>>> {
        match self {
            MemoryJournal::Empty { .. } => Ok(None),
            MemoryJournal::NonEmpty {
                id, range, data, ..
            } => range.satisfy(req).map_or_else(
                |err| match err {
                    SatisfyError::Pending => Ok(None),
                    SatisfyError::Impossible { .. } => Err(SyncError::FailedToPrepareRequest(err)),
                },
                |intersection_range| {
                    let offsets = range.intersection_offsets(&intersection_range);
                    Ok(Some(JournalPartial::new(
                        *id,
                        intersection_range,
                        MemoryScanCursor::new(&data[offsets]),
                    )))
                },
            ),
        }
    }

    fn sync_request(&mut self, id: JournalId) -> SyncResult<RequestedLsnRange> {
        if id != self.id() {
            return Err(SyncError::WrongJournal {
                from_id: id,
                self_id: self.id(),
            });
        }

        match self {
            MemoryJournal::Empty { .. } => Ok(RequestedLsnRange::new(0, DEFAULT_REQUEST_LEN)),
            MemoryJournal::NonEmpty { range, .. } => Ok(range.request_next(DEFAULT_REQUEST_LEN)),
        }
    }

    fn sync_receive<S>(&mut self, partial: JournalPartial<S>) -> SyncResult<LsnRange>
    where
        S: Cursor + io::Read,
    {
        if partial.id() != self.id() {
            return Err(SyncError::WrongJournal {
                from_id: partial.id(),
                self_id: self.id(),
            });
        }

        let partial_range = partial.range();
        let mut partial_data = Vec::with_capacity(partial.len());
        let mut cursor = partial.into_cursor();
        while cursor.advance()? {
            let mut buf = vec![];
            cursor.read_to_end(&mut buf)?;
            partial_data.push(buf);
        }

        match self {
            MemoryJournal::Empty { id, nextlsn } => {
                // ensure that nextlsn is contained by partial.range()
                if !partial_range.contains(*nextlsn) {
                    return Err(SyncError::RangesMustBeContiguous {
                        journal_debug: format!("{:?}", self),
                        partial_range,
                    });
                }

                *self = MemoryJournal::NonEmpty {
                    id: *id,
                    range: partial_range,
                    data: partial_data,
                };
                Ok(partial_range)
            }
            MemoryJournal::NonEmpty { range, data, .. } => {
                if !(range.intersects(&partial_range) || range.immediately_preceeds(&partial_range))
                {
                    return Err(SyncError::RangesMustBeContiguous {
                        journal_debug: format!("{:?}", self),
                        partial_range,
                    });
                }

                // insert partial.data into the journal at the correct position
                let offsets = range.intersection_offsets(&partial_range);
                if !offsets.is_empty() {
                    // intersection, so we need to replace the intersecting portion of the journal
                    *data = data[..offsets.start].to_vec();
                }
                data.extend(partial_data);

                // update the range
                *range = range.union(&partial_range);

                // return our new range
                Ok(*range)
            }
        }
    }
}
