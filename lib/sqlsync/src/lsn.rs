use std::{
    fmt::{Debug, Display},
    ops::Range,
};

use serde::{Deserialize, Serialize};

pub type Lsn = u64;

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[must_use]
pub enum LsnRange {
    Empty {
        /// nextlsn is the next lsn in the range
        nextlsn: Lsn,
    },
    NonEmpty {
        /// first marks the beginning of the range, inclusive.
        first: Lsn,
        /// last marks the end of the range, inclusive.
        last: Lsn,
    },
}

impl LsnRange {
    pub fn new(first: Lsn, last: Lsn) -> Self {
        assert!(first <= last, "first must be <= last");
        LsnRange::NonEmpty { first, last }
    }

    pub fn empty() -> Self {
        LsnRange::Empty { nextlsn: 0 }
    }

    pub fn empty_following(range: &LsnRange) -> Self {
        LsnRange::Empty {
            nextlsn: range.next(),
        }
    }

    /// returns an empty range with the nextlsn set to the first lsn of the target range
    pub fn empty_preceeding(range: &LsnRange) -> Self {
        LsnRange::Empty {
            nextlsn: match range {
                LsnRange::Empty { nextlsn } => *nextlsn,
                LsnRange::NonEmpty { first, .. } => *first,
            },
        }
    }

    pub fn is_empty(&self) -> bool {
        match self {
            LsnRange::Empty { .. } => true,
            LsnRange::NonEmpty { .. } => false,
        }
    }

    pub fn is_non_empty(&self) -> bool {
        !self.is_empty()
    }

    pub fn len(&self) -> usize {
        match self {
            LsnRange::Empty { .. } => 0,
            LsnRange::NonEmpty { first, last } => (last - first + 1) as usize,
        }
    }

    pub fn last(&self) -> Option<Lsn> {
        match self {
            LsnRange::Empty { .. } => None,
            LsnRange::NonEmpty { last, .. } => Some(*last),
        }
    }

    pub fn next(&self) -> Lsn {
        match self {
            LsnRange::Empty { nextlsn } => *nextlsn,
            LsnRange::NonEmpty { last, .. } => last + 1,
        }
    }

    pub fn contains(&self, lsn: Lsn) -> bool {
        match self {
            LsnRange::Empty { .. } => false,
            LsnRange::NonEmpty { first, last } => *first <= lsn && lsn <= *last,
        }
    }

    pub fn intersects(&self, other: &Self) -> bool {
        match (self, other) {
            (LsnRange::Empty { .. }, _) => false,
            (_, LsnRange::Empty { .. }) => false,
            (
                LsnRange::NonEmpty {
                    first: self_first,
                    last: self_last,
                },
                LsnRange::NonEmpty {
                    first: other_first,
                    last: other_last,
                },
            ) => self_last >= other_first && self_first <= other_last,
        }
    }

    pub fn immediately_preceeds(&self, other: &Self) -> bool {
        match (self, other) {
            (_, LsnRange::Empty { .. }) => false,
            (LsnRange::Empty { nextlsn }, LsnRange::NonEmpty { first, .. }) => *nextlsn == *first,
            (LsnRange::NonEmpty { last, .. }, LsnRange::NonEmpty { first, .. }) => {
                last + 1 == *first
            }
        }
    }

    pub fn immediately_follows(&self, other: &Self) -> bool {
        other.immediately_preceeds(self)
    }

    pub fn offset(&self, lsn: Lsn) -> Option<usize> {
        if self.contains(lsn) {
            match self {
                LsnRange::Empty { .. } => None,
                LsnRange::NonEmpty { first, .. } => Some(lsn.saturating_sub(*first) as usize),
            }
        } else {
            None
        }
    }

    pub fn intersection_offsets(&self, other: &Self) -> Range<usize> {
        if self.intersects(other) {
            match (self, other) {
                (
                    LsnRange::NonEmpty {
                        first: self_first,
                        last: self_last,
                    },
                    LsnRange::NonEmpty {
                        first: other_first,
                        last: other_last,
                    },
                ) => {
                    let start = std::cmp::max(*self_first, *other_first) - self_first;
                    let end = std::cmp::min(*self_last, *other_last) - self_first + 1;
                    start as usize..end as usize
                }
                (_, _) => 0..0,
            }
        } else {
            0..0
        }
    }

    // returns a new LsnRange with all lsns <= up_to removed
    pub fn trim_prefix(&self, up_to: Lsn) -> LsnRange {
        match self {
            LsnRange::Empty { nextlsn } => {
                let min_val = nextlsn.saturating_sub(1);
                assert!(up_to >= min_val, "up_to must be >= {}", min_val);
                LsnRange::Empty { nextlsn: up_to + 1 }
            }
            LsnRange::NonEmpty { first, last } => {
                if up_to >= *last {
                    LsnRange::Empty { nextlsn: up_to + 1 }
                } else if up_to < *first {
                    *self
                } else {
                    LsnRange::new(up_to + 1, *last)
                }
            }
        }
    }

    /// advance_first increments first
    /// returns self if already empty
    fn advance_first(&self) -> LsnRange {
        match self {
            LsnRange::Empty { .. } => *self,
            &LsnRange::NonEmpty { first, last } => {
                if first == last {
                    LsnRange::Empty { nextlsn: last + 1 }
                } else {
                    LsnRange::new(first + 1, last)
                }
            }
        }
    }

    /// remove_last decrements last
    /// returns self if already empty
    fn remove_last(&self) -> LsnRange {
        match self {
            LsnRange::Empty { .. } => *self,
            &LsnRange::NonEmpty { first, last } => {
                if first == last {
                    LsnRange::Empty { nextlsn: last + 1 }
                } else {
                    LsnRange::new(first, last - 1)
                }
            }
        }
    }

    pub fn extend_by(&self, len: u64) -> LsnRange {
        assert!(len > 0, "len must be > 0");
        match self {
            LsnRange::Empty { nextlsn } => LsnRange::new(*nextlsn, nextlsn + len - 1),
            LsnRange::NonEmpty { first, last } => LsnRange::new(*first, last + len),
        }
    }

    /// append the lsn to the range, panics if lsn is not the next lsn
    pub fn append(&self, lsn: Lsn) -> LsnRange {
        match self {
            LsnRange::Empty { nextlsn } => {
                assert_eq!(lsn, *nextlsn, "lsn must be the next lsn");
                LsnRange::new(*nextlsn, *nextlsn)
            }
            LsnRange::NonEmpty { first, last } => {
                assert_eq!(lsn, *last + 1, "lsn must be the next lsn");
                LsnRange::new(*first, *last + 1)
            }
        }
    }

    /// intersect returns the intersection between two ranges
    /// if the result is empty, its nextlsn will immediately follow self
    pub fn intersect(&self, other: &Self) -> Self {
        match (self, other) {
            (LsnRange::Empty { .. }, _) => *self,
            (LsnRange::NonEmpty { last, .. }, LsnRange::Empty { .. }) => {
                LsnRange::Empty { nextlsn: last + 1 }
            }
            (
                LsnRange::NonEmpty { first, last },
                LsnRange::NonEmpty {
                    first: other_first,
                    last: other_last,
                },
            ) => {
                if self.intersects(other) {
                    LsnRange::new(
                        std::cmp::max(*first, *other_first),
                        std::cmp::min(*last, *other_last),
                    )
                } else {
                    LsnRange::Empty { nextlsn: last + 1 }
                }
            }
        }
    }

    /// difference returns the difference between self and other
    /// represents self - other
    /// if the result is empty, its nextlsn will immediately follow self
    pub fn difference(&self, other: &Self) -> Self {
        match (self, other) {
            // if either is empty return self
            (LsnRange::Empty { .. }, _) => *self,
            (_, LsnRange::Empty { .. }) => *self,

            (
                &LsnRange::NonEmpty { first, last },
                &LsnRange::NonEmpty {
                    first: ofirst,
                    last: olast,
                },
            ) => {
                // No overlap: other is entirely before or entirely after self.
                if olast < first || ofirst > last {
                    *self
                } else if ofirst <= first && olast >= last {
                    // other completely covers self.
                    LsnRange::Empty { nextlsn: last + 1 }
                } else if ofirst <= first {
                    // other overlaps start of self.
                    LsnRange::NonEmpty {
                        first: olast + 1,
                        last,
                    }
                } else if olast >= last {
                    // other overlaps end of self.
                    LsnRange::NonEmpty {
                        first,
                        last: ofirst - 1,
                    }
                } else {
                    // other is entirely within self.
                    panic!("difference resulted in disjointed lsnrange")
                }
            }
        }
    }

    pub fn iter(&self) -> LsnIter {
        LsnIter { range: *self }
    }
}

impl Debug for LsnRange {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LsnRange::Empty { nextlsn } => f.debug_tuple("LsnRange::E").field(nextlsn).finish(),
            LsnRange::NonEmpty { first, last } => {
                f.debug_tuple("LsnRange").field(first).field(last).finish()
            }
        }
    }
}

impl Display for LsnRange {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        Debug::fmt(self, f)
    }
}

pub struct LsnIter {
    range: LsnRange,
}

impl Iterator for LsnIter {
    type Item = Lsn;

    fn next(&mut self) -> Option<Self::Item> {
        let lsn = match self.range {
            LsnRange::Empty { .. } => None,
            LsnRange::NonEmpty { first, .. } => Some(first),
        };
        self.range = self.range.advance_first();
        lsn
    }
}

impl DoubleEndedIterator for LsnIter {
    fn next_back(&mut self) -> Option<Self::Item> {
        let lsn = match self.range {
            LsnRange::Empty { .. } => None,
            LsnRange::NonEmpty { last, .. } => Some(last),
        };
        self.range = self.range.remove_last();
        lsn
    }
}

// write some tests for LsnRange
#[cfg(test)]
mod tests {
    use super::LsnRange;

    #[test]
    #[should_panic(expected = "first must be <= last")]
    fn lsnrange_invariant() {
        let _ = LsnRange::new(5, 0);
    }

    #[test]
    fn lsnrange_len() {
        assert_eq!(LsnRange::new(0, 0).len(), 1);
        assert_eq!(LsnRange::new(0, 1).len(), 2);
        assert_eq!(LsnRange::new(5, 10).len(), 6);
    }

    #[test]
    fn lsnrange_contains() {
        let range = LsnRange::new(5, 10);

        assert!(!range.contains(0));
        assert!(range.contains(5));
        assert!(range.contains(6));
        assert!(range.contains(10));
        assert!(!range.contains(11));
    }

    #[test]
    fn lsnrange_intersects() {
        let range = LsnRange::new(5, 10);

        macro_rules! t {
            ($other:expr, $intersection:expr, $offsets:expr) => {
                assert_eq!(
                    !range.intersects(&$other),
                    $offsets.is_empty(),
                    "checking intersects: {:?}, {:?}",
                    range,
                    $other
                );
                assert_eq!(
                    range.intersect(&$other),
                    $intersection,
                    "checking intersection: {:?}, {:?}",
                    range,
                    $other
                );
                assert_eq!(
                    range.intersection_offsets(&$other),
                    $offsets,
                    "checking intersection_offsets: {:?}, {:?}",
                    range,
                    $other
                );
            };
        }

        macro_rules! r {
            ($first:expr, $last:expr) => {
                LsnRange::new($first, $last)
            };
            (empty $nextlsn:expr) => {
                LsnRange::Empty { nextlsn: $nextlsn }
            };
        }

        t!(r!(0, 4), r!(empty 11), 0..0);
        t!(r!(0, 5), r!(5, 5), 0..1);
        t!(r!(0, 6), r!(5, 6), 0..2);
        t!(r!(0, 10), r!(5, 10), 0..6);
        t!(r!(0, 11), r!(5, 10), 0..6);
        t!(r!(5, 5), r!(5, 5), 0..1);
        t!(r!(5, 6), r!(5, 6), 0..2);
        t!(r!(5, 10), r!(5, 10), 0..6);
        t!(r!(5, 11), r!(5, 10), 0..6);
        t!(r!(9, 10), r!(9, 10), 4..6);
        t!(r!(10, 10), r!(10, 10), 5..6);
        t!(r!(10, 11), r!(10, 10), 5..6);
        t!(r!(11, 11), r!(empty 11), 0..0);
        t!(r!(20, 30), r!(empty 11), 0..0);
    }

    #[test]
    fn lsnrange_offset() {
        let range = LsnRange::new(5, 10);

        assert_eq!(range.offset(0), None);
        assert_eq!(range.offset(4), None);
        assert_eq!(range.offset(5), Some(0));
        assert_eq!(range.offset(6), Some(1));
        assert_eq!(range.offset(10), Some(5));
        assert_eq!(range.offset(11), None);
    }

    #[test]
    fn lsnrange_preceeds_follows() {
        let range = LsnRange::new(5, 10);

        macro_rules! t {
            ($other:expr, $result:literal) => {
                assert_eq!(range.immediately_preceeds(&$other), $result);
                assert_eq!($other.immediately_follows(&range), $result);
            };
        }

        t!(LsnRange::new(0, 4), false);
        t!(LsnRange::new(0, 5), false);
        t!(LsnRange::new(0, 6), false);
        t!(LsnRange::new(9, 10), false);
        t!(LsnRange::new(10, 10), false);
        t!(LsnRange::new(10, 11), false);
        t!(LsnRange::new(11, 11), true);
        t!(LsnRange::new(11, 12), true);
        t!(LsnRange::new(12, 12), false);
    }

    #[test]
    fn lsnrange_trim_prefix() {
        let range = LsnRange::new(5, 10);

        assert_eq!(range.trim_prefix(0), range);
        assert_eq!(range.trim_prefix(4), range);
        assert_eq!(range.trim_prefix(5), LsnRange::new(6, 10));
        assert_eq!(range.trim_prefix(6), LsnRange::new(7, 10));
        assert_eq!(range.trim_prefix(7), LsnRange::new(8, 10));
        assert_eq!(range.trim_prefix(8), LsnRange::new(9, 10));
        assert_eq!(range.trim_prefix(9), LsnRange::new(10, 10));
        assert_eq!(range.trim_prefix(10), LsnRange::Empty { nextlsn: 11 });
        assert_eq!(range.trim_prefix(20), LsnRange::Empty { nextlsn: 21 });
    }

    #[test]
    #[should_panic(expected = "len must be > 0")]
    fn lsnrange_extend_invariant() {
        let _ = LsnRange::new(5, 10).extend_by(0);
    }

    #[test]
    #[should_panic(expected = "lsn must be the next lsn")]
    fn lsnrange_append_invariant() {
        let _ = LsnRange::empty().append(5);
    }

    #[test]
    #[should_panic(expected = "lsn must be the next lsn")]
    fn lsnrange_append_invariant_nonempty() {
        let _ = LsnRange::new(5, 10).append(3);
    }

    #[test]
    fn lsnrange_extend() {
        let range = LsnRange::new(5, 10);
        assert_eq!(range.extend_by(1), LsnRange::new(5, 11));
        assert_eq!(range.extend_by(2), LsnRange::new(5, 12));

        let range = LsnRange::empty();
        assert_eq!(range.extend_by(1), LsnRange::new(0, 0));
        assert_eq!(range.extend_by(2), LsnRange::new(0, 1));

        let range = LsnRange::Empty { nextlsn: 5 };
        assert_eq!(range.extend_by(1), LsnRange::new(5, 5));
        assert_eq!(range.extend_by(2), LsnRange::new(5, 6));
    }

    #[test]
    fn lsnrange_append() {
        let range = LsnRange::new(5, 10);
        assert_eq!(range.append(11), LsnRange::new(5, 11));
        let range = LsnRange::empty();
        assert_eq!(range.append(0), LsnRange::new(0, 0));
        let range = LsnRange::Empty { nextlsn: 3 };
        assert_eq!(range.append(3), LsnRange::new(3, 3));
    }

    #[test]
    fn lsnrange_difference() {
        macro_rules! t {
            ($self:expr, $other:expr, $difference:expr) => {
                assert_eq!(
                    $self.difference(&$other),
                    $difference,
                    "checking difference: {:?}, {:?}",
                    $self,
                    $other
                );
            };
        }

        macro_rules! r {
            ($first:expr, $last:expr) => {
                LsnRange::new($first, $last)
            };
            (empty $nextlsn:expr) => {
                LsnRange::Empty { nextlsn: $nextlsn }
            };
        }

        // check either empty
        t!(r!(empty 0), r!(empty 0), r!(empty 0));
        t!(r!(empty 1), r!(empty 0), r!(empty 1));
        t!(r!(empty 1), r!(0, 10), r!(empty 1));
        t!(r!(5, 10), r!(empty 1), r!(5, 10));

        // check no overlap
        t!(r!(0, 4), r!(5, 10), r!(0, 4));
        t!(r!(5, 10), r!(0, 4), r!(5, 10));

        // check other completely covers self
        t!(r!(0, 4), r!(0, 10), r!(empty 5));
        t!(r!(0, 10), r!(0, 10), r!(empty 11));
        t!(r!(3, 7), r!(0, 11), r!(empty 8));

        // check other overlaps start of self
        t!(r!(0, 4), r!(0, 3), r!(4, 4));
        t!(r!(5, 10), r!(0, 6), r!(7, 10));

        // check other overlaps end of self
        t!(r!(0, 4), r!(3, 4), r!(0, 2));
        t!(r!(0, 4), r!(4, 4), r!(0, 3));
        t!(r!(5, 10), r!(8, 10), r!(5, 7));
    }

    #[test]
    #[should_panic(expected = "difference resulted in disjointed lsnrange")]
    fn lsnrange_difference_disjoint_panics() {
        let _ = LsnRange::new(5, 10).difference(&LsnRange::new(6, 9));
    }

    #[test]
    fn lsnrange_advance_first() {
        assert_eq!(
            LsnRange::new(5, 5).advance_first(),
            LsnRange::Empty { nextlsn: 6 }
        );
        assert_eq!(LsnRange::new(5, 6).advance_first(), LsnRange::new(6, 6));
        assert_eq!(LsnRange::new(5, 10).advance_first(), LsnRange::new(6, 10));
        assert_eq!(
            LsnRange::empty().advance_first(),
            LsnRange::Empty { nextlsn: 0 }
        );
        assert_eq!(
            LsnRange::Empty { nextlsn: 5 }.advance_first(),
            LsnRange::Empty { nextlsn: 5 }
        );
    }

    #[test]
    fn lsnrange_remove_last() {
        assert_eq!(
            LsnRange::new(5, 5).remove_last(),
            LsnRange::Empty { nextlsn: 6 }
        );
        assert_eq!(LsnRange::new(5, 6).remove_last(), LsnRange::new(5, 5));
        assert_eq!(LsnRange::new(5, 10).remove_last(), LsnRange::new(5, 9));
        assert_eq!(
            LsnRange::empty().remove_last(),
            LsnRange::Empty { nextlsn: 0 }
        );
        assert_eq!(
            LsnRange::Empty { nextlsn: 5 }.remove_last(),
            LsnRange::Empty { nextlsn: 5 }
        );
    }

    #[test]
    fn lsnrange_iter() {
        let range = LsnRange::new(5, 10);
        let mut iter = range.iter();
        assert_eq!(iter.next(), Some(5));
        assert_eq!(iter.next(), Some(6));
        assert_eq!(iter.next(), Some(7));
        assert_eq!(iter.next(), Some(8));
        assert_eq!(iter.next(), Some(9));
        assert_eq!(iter.next(), Some(10));
        assert_eq!(iter.next(), None);

        let mut iter = range.iter().rev();
        assert_eq!(iter.next(), Some(10));
        assert_eq!(iter.next(), Some(9));
        assert_eq!(iter.next(), Some(8));
        assert_eq!(iter.next(), Some(7));
        assert_eq!(iter.next(), Some(6));
        assert_eq!(iter.next(), Some(5));
        assert_eq!(iter.next(), None);

        let range = LsnRange::empty();
        let mut iter = range.iter();
        assert_eq!(iter.next(), None);
        let mut iter = range.iter().rev();
        assert_eq!(iter.next(), None);
    }
}
