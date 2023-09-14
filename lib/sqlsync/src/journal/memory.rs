use std::fmt::{Debug, Formatter};
use std::io;

use crate::lsn::{Lsn, LsnIter, LsnRange};
use crate::{JournalError, JournalFactory, Serializable};

use super::{Cursor, Journal, JournalId, JournalResult, Scannable};
use crate::replication::{ReplicationDestination, ReplicationError, ReplicationSource};

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

impl MemoryJournal {
    pub fn open(id: JournalId) -> JournalResult<Self> {
        Ok(MemoryJournal {
            id,
            range: LsnRange::empty(),
            data: vec![],
        })
    }
}

pub struct MemoryJournalFactory;

impl JournalFactory<MemoryJournal> for MemoryJournalFactory {
    fn open(&self, id: JournalId) -> JournalResult<MemoryJournal> {
        MemoryJournal::open(id)
    }
}

impl Journal for MemoryJournal {
    type Factory = MemoryJournalFactory;

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

impl Scannable for MemoryJournal {
    type Reader<'a> = &'a [u8]
    where
        Self: 'a;

    fn scan<'a>(&'a self) -> Cursor<'a, Self, LsnIter> {
        Cursor::new(self, self.range.iter())
    }

    fn scan_range<'a>(&'a self, range: LsnRange) -> Cursor<'a, Self, LsnIter> {
        let intersection = self.range.intersect(&range);
        Cursor::new(self, intersection.iter())
    }

    fn get<'a>(&'a self, lsn: Lsn) -> io::Result<Option<Self::Reader<'a>>> {
        Ok(self
            .range
            .offset(lsn)
            .map(|offset| self.data[offset].as_slice()))
    }
}

impl ReplicationSource for MemoryJournal {
    type Reader<'a> = &'a [u8]
    where
        Self: 'a;

    fn source_id(&self) -> JournalId {
        self.id()
    }

    fn source_range(&self) -> LsnRange {
        self.range()
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

        let accepted_range = if self.range.is_empty() {
            // if we have no range, then we reset to the incoming lsn
            LsnRange::new(lsn, lsn)
        } else {
            // accept any lsn in our current range or immediately following
            self.range.extend_by(1)
        };

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
