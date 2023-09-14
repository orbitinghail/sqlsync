use std::io::Cursor;

use js_sys::Uint8Array;
use sqlsync::{replication::ReplicationDestination, JournalId, Lsn, LsnRange};
use wasm_bindgen::JsValue;
use worker::*;

const RANGE_KEY: &str = "RANGE";

pub struct Persistence {
    /// The range of lsns that have been written to storage
    range: LsnRange,
    storage: Storage,
}

impl Persistence {
    pub async fn init(mut storage: Storage) -> Result<Self> {
        let range = match storage.get::<LsnRange>(RANGE_KEY).await {
            Ok(range) => range,
            Err(_) => {
                let range = LsnRange::empty();
                storage.put(RANGE_KEY, &range).await?;
                range
            }
        };
        Ok(Self { range, storage })
    }

    /// the next lsn that should be written to storage
    pub fn expected_lsn(&self) -> Lsn {
        self.range.next()
    }

    pub async fn write_lsn(&mut self, lsn: Lsn, frame: Vec<u8>) -> Result<()> {
        let obj = js_sys::Object::new();

        // get the new range, assuming the write goes through
        let new_range = self.range.append(lsn);

        // convert our range into a jsvalue
        let range = serde_wasm_bindgen::to_value(&new_range)
            .map_err(|e| Error::RustError(e.to_string()))?;

        js_sys::Reflect::set(&obj, &JsValue::from_str(RANGE_KEY), &range)?;

        // convert frame into a uint8array
        let uint8_array = Uint8Array::from(frame.as_slice());
        let key = format!("lsn-{}", lsn);
        js_sys::Reflect::set(&obj, &JsValue::from_str(&key), &uint8_array)?;

        // write to storage
        self.storage.put_multiple_raw(obj).await?;

        // update our in-memory range
        self.range = new_range;
        Ok(())
    }

    pub async fn replay<T: ReplicationDestination>(
        &self,
        id: JournalId,
        dest: &mut T,
    ) -> Result<()> {
        for lsn in 0..self.range.next() {
            console_log!("replaying lsn {}", lsn);
            let key = format!("lsn-{}", lsn);
            let mut frame = Cursor::new(self.storage.get::<serde_bytes::ByteBuf>(&key).await?);
            dest.write_lsn(id, lsn, &mut frame)
                .map_err(|e| Error::RustError(e.to_string()))?;
        }
        Ok(())
    }
}
