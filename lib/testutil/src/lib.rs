pub use assert_matches::assert_matches;
pub use assert_panic::assert_panic;

#[macro_export]
macro_rules! assert_ok {
    ( $e:expr , $($arg:tt)*) => {
        assert_matches!($e, Ok(_), $($arg)*)
    };
}
