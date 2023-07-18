extern crate wasm_bindgen_test;
use wasm_bindgen_test::*;

use testutil::assert_ok;

wasm_bindgen_test_configure!(run_in_browser);

#[wasm_bindgen_test]
fn pass() {
    assert_ok!(sqlsync_wasm::run(), "expected run() to succeed");
}
