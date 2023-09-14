/// has_sorted_intersection returns true if the two iterators have an intersection
/// requires that both iterators are sorted
pub fn has_sorted_intersection<'a, I, J, T>(a: I, b: J) -> bool
where
    T: Ord + Eq,
    I: IntoIterator<Item = T>,
    J: IntoIterator<Item = T>,
{
    let mut a = a.into_iter();
    let mut b = b.into_iter();
    let mut a_val = a.next();
    let mut b_val = b.next();

    while let (Some(a_cur), Some(b_cur)) = (a_val.as_ref(), b_val.as_ref()) {
        if a_cur == b_cur {
            return true;
        }
        if a_cur < b_cur {
            a_val = a.next();
        } else {
            b_val = b.next();
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use crate::iter::has_sorted_intersection;

    #[test]
    fn test_has_sorted_intersection() {
        // write a simple test for has_sorted_intersection
        let empty = [0; 0];

        assert!(has_sorted_intersection([0], [0]));
        assert!(has_sorted_intersection([0, 1], [1, 2]),);
        assert!(has_sorted_intersection([1, 2, 3], [1, 2, 3]));
        assert!(has_sorted_intersection([0, 0], [0, 1]));
        assert!(has_sorted_intersection([1, 2, 2, 3], [0, 0, 0, 3]));

        assert!(!has_sorted_intersection([1, 2], [3, 4]));
        assert!(!has_sorted_intersection([1, 2], [10]));
        assert!(!has_sorted_intersection([1, 2, 2, 3], [0]));

        assert!(!has_sorted_intersection([1, 2], empty));
        assert!(!has_sorted_intersection(empty, empty));
        assert!(!has_sorted_intersection(empty, [1, 2]));
    }
}
