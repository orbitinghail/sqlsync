#[cfg(not(target_family = "wasm"))]
pub fn unix_timestamp_milliseconds() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("time went backwards")
        .as_millis() as i64
}

#[cfg(target_family = "wasm")]
pub fn unix_timestamp_milliseconds() -> i64 {
    js_sys::Date::now() as i64
}
