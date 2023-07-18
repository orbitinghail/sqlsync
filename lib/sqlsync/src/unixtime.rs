pub trait UnixTime: Clone {
    // return the number of milliseconds which have elapsed since the unix epoch
    fn unix_timestamp_milliseconds(&self) -> i64;
}

#[derive(Clone)]
pub struct SystemUnixTime {}

impl SystemUnixTime {
    pub fn new() -> Self {
        Self {}
    }
}

impl UnixTime for SystemUnixTime {
    fn unix_timestamp_milliseconds(&self) -> i64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("time went backwards")
            .as_millis() as i64
    }
}
