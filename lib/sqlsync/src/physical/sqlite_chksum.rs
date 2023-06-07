use byteorder::ByteOrder;

pub struct SqliteChksum<E: ByteOrder> {
    s0: u32,
    s1: u32,
    _phantom: std::marker::PhantomData<E>,
}

impl<E: ByteOrder> SqliteChksum<E> {
    pub fn new(s0: u32, s1: u32) -> Self {
        SqliteChksum {
            s0,
            s1,
            _phantom: std::marker::PhantomData,
        }
    }

    pub fn update(&mut self, b: &[u8]) {
        assert!(
            b.len() % 8 == 0,
            "wal_checksum: b.len() must be a multiple of 8"
        );

        let s0 = &mut self.s0;
        let s1 = &mut self.s1;
        for i in (0..b.len()).step_by(8) {
            *s0 = s0.wrapping_add(s1.wrapping_add(E::read_u32(&b[i..])));
            *s1 = s1.wrapping_add(s0.wrapping_add(E::read_u32(&b[i + 4..])));
        }
    }

    pub fn read(&self) -> (u32, u32) {
        (self.s0, self.s1)
    }
}

pub fn sqlite_chksum<E: ByteOrder>(s0: u32, s1: u32, b: &[u8]) -> (u32, u32) {
    let mut chksum = SqliteChksum::<E>::new(s0, s1);
    chksum.update(b);
    chksum.read()
}
