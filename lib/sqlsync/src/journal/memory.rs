use std::fmt::{Debug, Formatter};
use std::io;

use crate::lsn::{Lsn, LsnRange};
use crate::positioned_io::PositionedReader;
use crate::{JournalError, ScanError, Serializable};

use super::replication::{ReplicationDestination, ReplicationError, ReplicationSource};
use super::{Cursor, Journal, JournalId, JournalResult, Scannable};

pub struct MemoryJournal {
    id: JournalId,
    range: LsnRange,
    data: Vec<Vec<u8>>,
}

impl Debug for MemoryJournal {
    fn fmt(&self, f: &mut Formatter) -> std::fmt::Result {
        f.debug_tuple("MemoryJournal")
            .field(&self.id)
            .field(&self.range)
            .finish()
    }
}

impl Journal for MemoryJournal {
    fn open(id: JournalId) -> JournalResult<Self> {
        Ok(MemoryJournal {
            id,
            range: LsnRange::empty(),
            data: vec![],
        })
    }

    fn id(&self) -> JournalId {
        self.id
    }

    fn range(&self) -> LsnRange {
        self.range
    }

    fn append(&mut self, obj: impl Serializable) -> JournalResult<()> {
        // serialize the entry
        let mut entry: Vec<u8> = Vec::new();
        obj.serialize_into(&mut entry)
            .map_err(|err| JournalError::SerializationError(err))?;

        // update the journal
        self.data.push(entry);
        self.range = self.range.extend_by(1);

        Ok(())
    }

    fn drop_prefix(&mut self, up_to: Lsn) -> JournalResult<()> {
        let remaining_range = self.range.trim_prefix(up_to);
        let offsets = self.range.intersection_offsets(&remaining_range);
        self.data = self.data[offsets].to_vec();
        self.range = remaining_range;
        Ok(())
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
    fn advance(&mut self) -> std::result::Result<bool, ScanError> {
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
        MemoryScanCursor::new(&self.data)
    }

    fn scan_range<'a>(&'a self, range: LsnRange) -> Self::Cursor<'a> {
        let offsets = self.range.intersection_offsets(&range);
        MemoryScanCursor::new(&self.data[offsets])
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
        match self.range.offset(lsn) {
            None => Ok(None),
            Some(offset) => Ok(Some(&self.data[offset][..])),
        }
    }
}

impl ReplicationDestination for MemoryJournal {
    fn range(&mut self, id: JournalId) -> Result<LsnRange, ReplicationError> {
        if id != self.id {
            return Err(ReplicationError::UnknownJournal(id));
        }
        Ok(self.range)
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

        // accept any lsn in our current range or immediately following
        let accepted_range = self.range.extend_by(1);

        if accepted_range.contains(lsn) {
            let mut frame_data = Vec::new();
            reader.read_to_end(&mut frame_data)?;

            // store frame into self.data
            match self.range.offset(lsn) {
                Some(offset) => {
                    self.data[offset] = frame_data
                    // no need to update range since this was an intersection
                }
                None => {
                    self.data.push(frame_data);
                    // update our range to include the new lsn
                    self.range = accepted_range;
                }
            }

            Ok(())
        } else {
            Err(ReplicationError::NonContiguousLsn {
                received: lsn,
                range: accepted_range,
            })
        }
    }
}
