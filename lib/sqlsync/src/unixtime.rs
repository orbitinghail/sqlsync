pub trait UnixTime: Clone {
    fn unix_timestamp(&self) -> i64;
}

#[derive(Clone)]
pub struct SystemUnixTime {}

impl SystemUnixTime {
    pub fn new() -> Self {
        Self {}
    }
}

impl UnixTime for SystemUnixTime {
    fn unix_timestamp(&self) -> i64 {
        time::OffsetDateTime::now_utc().unix_timestamp()
    }
}
