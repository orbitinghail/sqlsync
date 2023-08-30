use std::fmt::{Debug, Formatter};
use std::io;

use crate::lsn::{Lsn, LsnRange};
use crate::positioned_io::PositionedReader;
use crate::{JournalError, Serializable};

use super::replication::{ReplicationDestination, ReplicationError, ReplicationSource};
use super::{Cursor, Journal, JournalId, JournalResult, Scannable};

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

    fn range(&self) -> Option<LsnRange> {
        match self {
            MemoryJournal::Empty { .. } => None,
            MemoryJournal::NonEmpty { range, .. } => Some(*range),
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

    fn into_rev(self) -> Self {
        self.reverse()
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

impl ReplicationSource for MemoryJournal {
    type Reader<'a> = &'a [u8]
    where
        Self: 'a;

    fn source_id(&self) -> JournalId {
        self.id()
    }

    fn read_lsn<'a>(&'a self, lsn: Lsn) -> io::Result<Option<Self::Reader<'a>>> {
        match self {
            MemoryJournal::Empty { .. } => Ok(None),
            MemoryJournal::NonEmpty { range, data, .. } => match range.offset(lsn) {
                None => Ok(None),
                Some(offset) => Ok(Some(&data[offset][..])),
            },
        }
    }
}

impl ReplicationDestination for MemoryJournal {
    fn range(&mut self, id: JournalId) -> Result<Option<LsnRange>, ReplicationError> {
        if id != self.id() {
            return Err(ReplicationError::UnknownJournal(id));
        }
        match self {
            MemoryJournal::Empty { .. } => Ok(None),
            MemoryJournal::NonEmpty { range, .. } => Ok(Some(*range)),
        }
    }

    fn write_lsn<R>(
        &mut self,
        id: JournalId,
        lsn: Lsn,
        reader: &mut R,
    ) -> Result<(), ReplicationError>
    where
        R: io::Read,
    {
        if id != self.id() {
            return Err(ReplicationError::UnknownJournal(id));
        }

        let mut frame_data = Vec::new();
        reader.read_to_end(&mut frame_data)?;

        match self {
            MemoryJournal::Empty { id, nextlsn } => {
                if lsn != *nextlsn {
                    return Err(ReplicationError::NonContiguousLsn {
                        received: lsn,
                        range: LsnRange::new(*nextlsn, *nextlsn),
                    });
                }
                *self = MemoryJournal::NonEmpty {
                    id: *id,
                    range: LsnRange::new(lsn, lsn),
                    data: vec![frame_data],
                };
                Ok(())
            }
            MemoryJournal::NonEmpty { range, data, .. } => {
                // accept any lsn in our current range or immediately following
                let accepted_range = range.extend_by(1);
                if !accepted_range.contains(lsn) {
                    return Err(ReplicationError::NonContiguousLsn {
                        received: lsn,
                        range: accepted_range,
                    });
                }
                match range.offset(lsn) {
                    Some(offset) => {
                        // intersection, replace specified lsn
                        data[offset] = frame_data;
                        // no need to update range
                    }
                    None => {
                        // no intersection, append to the end
                        data.push(frame_data);
                        // update our range to include the new lsn
                        *range = accepted_range;
                    }
                }
                Ok(())
            }
        }
    }
}
