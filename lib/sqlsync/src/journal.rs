use std::fmt::{Debug, Formatter};
use std::io;

use crate::lsn::{Lsn, LsnRange, RequestedLsnRange, SatisfyError};
use crate::positioned_io::PositionedReader;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum JournalError {
    #[error("failed to open journal, error: {0}")]
    FailedToOpenJournal(#[source] anyhow::Error),

    #[error("refusing to sync from journal id {partial_id} into journal id {self_id}")]
    WrongJournal {
        partial_id: JournalId,
        self_id: JournalId,
    },

    #[error("journal range {journal_debug} does not intersect or preceed partial range {partial_range:?}")]
    RangesMustBeContiguous {
        journal_debug: String,
        partial_range: LsnRange,
    },

    #[error("failed to prepare journal partial from request: {0}")]
    FailedToPrepareRequest(#[source] SatisfyError),

    #[error("failed to serialize object")]
    SerializationError(#[source] anyhow::Error),
}

pub type JournalResult<T> = std::result::Result<T, JournalError>;

pub type JournalId = i64;

pub struct JournalPartial<I: DoubleEndedIterator> {
    pub id: JournalId,
    pub range: LsnRange,
    iter: I,
}

impl<I: DoubleEndedIterator> IntoIterator for JournalPartial<I> {
    type Item = I::Item;
    type IntoIter = I;

    fn into_iter(self) -> Self::IntoIter {
        self.iter
    }
}

pub trait Serializable {
    /// serialize the object into the given writer
    fn serialize_into<W: io::Write>(&self, writer: &mut W) -> anyhow::Result<()>;
}

pub trait Deserializable: Sized {
    /// deserialize the object from the given reader
    fn deserialize_from<R: PositionedReader>(reader: R) -> anyhow::Result<Self>;
}

pub trait Journal: Debug + Sized {
    type Entry: PositionedReader;

    type Iter<'a>: DoubleEndedIterator<Item = &'a Self::Entry>
    where
        <Self as Journal>::Entry: 'a;

    fn open(id: JournalId) -> JournalResult<Self>;

    // TODO: eventually this needs to be a UUID of some kind
    fn id(&self) -> JournalId;

    /// append a new journal entry, and then write to it
    fn append(&mut self, obj: impl Serializable) -> JournalResult<()>;

    /// iterate over journal entries
    fn iter(&self) -> JournalResult<Self::Iter<'_>>;

    fn iter_range(&self, range: LsnRange) -> JournalResult<Self::Iter<'_>>;

    /// sync
    fn sync_prepare(
        &self,
        req: RequestedLsnRange,
    ) -> JournalResult<Option<JournalPartial<Self::Iter<'_>>>>;

    fn sync_receive(&mut self, partial: JournalPartial<Self::Iter<'_>>) -> JournalResult<LsnRange>;

    /// drop the journal's prefix
    fn drop_prefix(&mut self, up_to: Lsn) -> JournalResult<()>;
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
    type Entry = Vec<u8>;
    type Iter<'a> = std::slice::Iter<'a, Vec<u8>>;

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

    fn iter(&self) -> JournalResult<Self::Iter<'_>> {
        match self {
            Self::Empty { .. } => Ok([].iter()),
            Self::NonEmpty { data, .. } => Ok(data.iter()),
        }
    }

    fn iter_range(&self, range: LsnRange) -> JournalResult<Self::Iter<'_>> {
        match self {
            Self::Empty { .. } => Ok([].iter()),
            Self::NonEmpty {
                range: journal_range,
                data,
                ..
            } => journal_range.intersect(&range).map_or_else(
                || Ok([].iter()),
                |intersection_range| {
                    let offsets = journal_range.intersection_offsets(&intersection_range);
                    Ok(data[offsets].iter())
                },
            ),
        }
    }

    fn sync_prepare(
        &self,
        req: RequestedLsnRange,
    ) -> JournalResult<Option<JournalPartial<Self::Iter<'_>>>> {
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
                    Ok(Some(JournalPartial {
                        id: *id,
                        range: intersection_range,
                        iter: data[offsets].iter(),
                    }))
                },
            ),
        }
    }

    fn sync_receive(&mut self, partial: JournalPartial<Self::Iter<'_>>) -> JournalResult<LsnRange> {
        if partial.id != self.id() {
            return Err(JournalError::WrongJournal {
                partial_id: partial.id,
                self_id: self.id(),
            });
        }

        let partial_range = partial.range;
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
                    data: partial.into_iter().cloned().collect(),
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
                if offsets.is_empty() {
                    // no intersection, so we can just append the partial to this journal
                    data.extend(partial.into_iter().cloned());
                } else {
                    // intersection, so we need to replace the intersecting portion of the journal
                    *data = data[..offsets.start]
                        .iter()
                        .chain(partial.into_iter())
                        .cloned()
                        .collect();
                }

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
