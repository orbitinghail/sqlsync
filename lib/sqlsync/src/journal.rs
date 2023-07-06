use anyhow::Result;
use std::fmt::Debug;

use crate::lsn::{Lsn, LsnRange, RequestedLsnRange};

pub struct JournalPartial<'a, T> {
    range: LsnRange,
    data: &'a [T],
}

impl<'a, T> JournalPartial<'a, T> {
    pub fn new(range: LsnRange, data: &'a [T]) -> Self {
        assert!(
            range.len() == data.len(),
            "range and data must be the same length"
        );
        Self { range, data }
    }
}

pub enum Journal<T>
where
    T: Clone,
{
    Empty {
        /// We need to keep track of the next lsn to use if we are empty.
        /// this is because journals can trim their prefix after it's been safely replicated elsewhere
        nextlsn: Lsn,
    },
    NonEmpty {
        /// The range of lsns covered by this journal.
        range: LsnRange,
        /// The data contained in this journal.
        /// data[0] corresponds to range.first, data[1] to range.first + 1, etc.
        data: Vec<T>,
    },
}

impl<T: Clone> Debug for Journal<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Journal::Empty { nextlsn } => f.debug_tuple("Journal::Empty").field(nextlsn).finish(),
            Journal::NonEmpty { range, .. } => f.debug_tuple("Journal").field(range).finish(),
        }
    }
}

impl<T> Journal<T>
where
    T: Clone,
{
    pub fn new() -> Self {
        Journal::Empty { nextlsn: 0 }
    }

    /// Append a single entry to the journal.
    pub fn append(&mut self, entry: T) {
        match self {
            Journal::Empty { nextlsn } => {
                *self = Journal::NonEmpty {
                    range: LsnRange::new(*nextlsn, *nextlsn),
                    data: vec![entry],
                };
            }
            Journal::NonEmpty { range, data } => {
                data.push(entry);
                *range = range.extend_by(1);
            }
        }
    }

    pub fn sync_request(&self, max_sync: usize) -> RequestedLsnRange {
        match self {
            Journal::Empty { nextlsn } => RequestedLsnRange::new(*nextlsn, max_sync),
            Journal::NonEmpty { range, .. } => RequestedLsnRange::new(range.last() + 1, max_sync),
        }
    }

    /// Satisfy a request for a range of LSNs
    pub fn sync_prepare<'a>(&'a self, req: RequestedLsnRange) -> Option<JournalPartial<'a, T>> {
        match self {
            Journal::Empty { .. } => None,
            Journal::NonEmpty { range, data } => {
                if let Some(partial_range) = range.satisfy(req) {
                    let offsets = range.intersection_offsets(&partial_range);
                    Some(JournalPartial::new(partial_range, &data[offsets]))
                } else {
                    None
                }
            }
        }
    }

    /// Merge a partial into the journal starting at partial.start and possibly extending the journal.
    /// The journal must intersect or meet the partial range.
    pub fn sync_receive(&mut self, partial: JournalPartial<T>) -> Result<LsnRange> {
        log::debug!("sync_receive({:?})", partial.range);
        match self {
            Journal::Empty { .. } => {
                *self = Journal::NonEmpty {
                    range: partial.range,
                    data: partial.data.to_vec(),
                };
                Ok(partial.range)
            }
            Journal::NonEmpty { range, data } => {
                if !(range.intersects(&partial.range) || range.immediately_preceeds(&partial.range))
                {
                    return Err(anyhow::anyhow!(
                        "journal range {:?} does not intersect or preceed partial range {:?}",
                        range,
                        partial.range,
                    ));
                }

                // insert partial.data into the journal at the correct position
                let offsets = range.intersection_offsets(&partial.range);
                if offsets.is_empty() {
                    // no intersection, so we can just append the partial to this journal
                    data.extend_from_slice(partial.data);
                } else {
                    // intersection, so we need to replace the intersecting portion of the journal
                    *data = [&data[..offsets.start], partial.data].concat();
                }

                // update the range
                *range = range.union(&partial.range);

                // return our new range
                Ok(*range)
            }
        }
    }

    /// Remove all entries from the journal in the provided range.
    /// The provided range must intersect or preceed the journal's start.
    pub fn remove_up_to(&mut self, lsn: Lsn) {
        match self {
            Journal::Empty { .. } => {}
            Journal::NonEmpty { range, data } => {
                if let Some(remaining_range) = range.trim_prefix(lsn) {
                    let offsets = range.intersection_offsets(&remaining_range);
                    *data = data[offsets].to_vec();
                    *range = remaining_range;
                } else {
                    *self = Journal::Empty {
                        nextlsn: range.last() + 1,
                    };
                }
            }
        }
    }

    pub fn iter(&self) -> impl DoubleEndedIterator<Item = &T> {
        match self {
            Journal::Empty { .. } => [].iter(),
            Journal::NonEmpty { data, .. } => data.iter(),
        }
    }

    /// Iterate over the entries in the journal in the given LsnRange
    pub fn iter_range(&self, range: LsnRange) -> impl DoubleEndedIterator<Item = &T> {
        match self {
            Journal::Empty { .. } => [].iter(),
            Journal::NonEmpty {
                range: journal_range,
                data,
            } => {
                let offsets = journal_range.intersection_offsets(&range);
                data[offsets].iter()
            }
        }
    }
}
