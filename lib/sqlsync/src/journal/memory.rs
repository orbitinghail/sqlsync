use std::collections::VecDeque;
use std::fmt::{Debug, Formatter};

use crate::lsn::{Lsn, LsnRange, RequestedLsnRange, SatisfyError};
use crate::positioned_io::PositionedReader;
use crate::Serializable;

use super::error::{JournalError, JournalResult};
use super::{Journal, JournalId, JournalIterator};

pub struct MemoryJournalIter {
    id: JournalId,
    range: Option<LsnRange>,
    data: VecDeque<Vec<u8>>,
}

impl MemoryJournalIter {
    fn empty(id: JournalId) -> Self {
        Self {
            id,
            range: None,
            data: VecDeque::new(),
        }
    }

    fn new(id: JournalId, range: LsnRange, data: Vec<Vec<u8>>) -> Self {
        Self {
            id,
            range: Some(range),
            data: data.into(),
        }
    }
}

impl JournalIterator for MemoryJournalIter {
    type Entry = Vec<u8>;

    fn id(&self) -> JournalId {
        self.id
    }

    fn range(&self) -> Option<LsnRange> {
        self.range
    }
}

impl Iterator for MemoryJournalIter {
    type Item = Vec<u8>;

    fn next(&mut self) -> Option<Self::Item> {
        self.range = self.range.and_then(|range| range.remove_first());
        self.data.pop_front()
    }
}

impl DoubleEndedIterator for MemoryJournalIter {
    fn next_back(&mut self) -> Option<Self::Item> {
        self.range = self.range.and_then(|range| range.remove_last());
        self.data.pop_back()
    }
}

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
    type Iter = MemoryJournalIter;

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

    fn iter(&self) -> JournalResult<Self::Iter> {
        match self {
            Self::Empty { id, .. } => Ok(MemoryJournalIter::empty(*id)),
            Self::NonEmpty {
                id, range, data, ..
            } => Ok(MemoryJournalIter::new(*id, *range, data.to_vec())),
        }
    }

    fn iter_range(&self, range: LsnRange) -> JournalResult<Self::Iter> {
        match self {
            Self::Empty { id, .. } => Ok(MemoryJournalIter::empty(*id)),
            Self::NonEmpty {
                id,
                range: journal_range,
                data,
                ..
            } => journal_range.intersect(&range).map_or_else(
                || Ok(MemoryJournalIter::empty(*id)),
                |intersection_range| {
                    let offsets = journal_range.intersection_offsets(&intersection_range);
                    Ok(MemoryJournalIter::new(
                        *id,
                        intersection_range,
                        data[offsets].to_vec(),
                    ))
                },
            ),
        }
    }

    fn sync_prepare(&self, req: RequestedLsnRange) -> JournalResult<Option<Self::Iter>> {
        match self {
            MemoryJournal::Empty { .. } => Ok(None),
            MemoryJournal::NonEmpty {
                id, range, data, ..
            } => range.satisfy(req).map_or_else(
                |err| match err {
                    SatisfyError::Pending => Ok(None),
                    SatisfyError::Impossible { .. } => {
                        Err(JournalError::FailedToPrepareRequest(err))
                    }
                },
                |intersection_range| {
                    let offsets = range.intersection_offsets(&intersection_range);
                    Ok(Some(MemoryJournalIter::new(
                        *id,
                        intersection_range,
                        data[offsets].to_vec(),
                    )))
                },
            ),
        }
    }

    fn sync_receive(&mut self, partial: impl JournalIterator) -> JournalResult<LsnRange> {
        if partial.id() != self.id() {
            return Err(JournalError::WrongJournal {
                partial_id: partial.id(),
                self_id: self.id(),
            });
        }

        let partial_range = partial.range().ok_or(JournalError::EmptyPartial)?;
        let partial_data = partial.map(|e| e.read_all()).collect::<Result<_, _>>()?;

        match self {
            MemoryJournal::Empty { id, nextlsn } => {
                // ensure that nextlsn is contained by partial.range()
                if !partial_range.contains(*nextlsn) {
                    return Err(JournalError::RangesMustBeContiguous {
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
                    return Err(JournalError::RangesMustBeContiguous {
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
