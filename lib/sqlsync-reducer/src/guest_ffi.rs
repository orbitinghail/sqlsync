use std::{collections::BTreeMap, mem::MaybeUninit, panic, sync::Once};

use serde::{de::DeserializeOwned, Serialize};

use crate::types::LogRecord;

pub type FFIBuf = Vec<u8>;
pub type FFIBufPtr = *mut u8;
pub type FFIBufLen = u32;

pub fn fbm() -> &'static mut FFIBufManager {
    static mut SINGLETON: MaybeUninit<FFIBufManager> = MaybeUninit::uninit();
    static ONCE: Once = Once::new();
    unsafe {
        ONCE.call_once(|| {
            let singleton = FFIBufManager::default();
            SINGLETON.write(singleton);
        });
        SINGLETON.assume_init_mut()
    }
}

#[derive(Default)]
pub struct FFIBufManager {
    // map from pointer to buffer to length of buffer
    bufs: BTreeMap<FFIBufPtr, FFIBufLen>,
}

impl FFIBufManager {
    pub fn alloc(&mut self, len: FFIBufLen) -> FFIBufPtr {
        let mut buf = Vec::with_capacity(len as usize);
        let ptr = buf.as_mut_ptr();
        self.bufs.insert(ptr, len);
        std::mem::forget(buf);
        ptr
    }

    /// frees the memory pointed to by ptr
    ///
    /// # Safety
    /// The pointer must have been allocated by FFIBufManager::alloc.
    pub unsafe fn dealloc(&mut self, ptr: FFIBufPtr) {
        self.consume(ptr);
        // immediately drops the vec, freeing the memory
    }

    pub fn length(&self, ptr: FFIBufPtr) -> FFIBufLen {
        *self.bufs.get(&ptr).unwrap()
    }

    /// consumes the buffer pointed to by ptr and returns a Vec<u8> with the same contents.
    ///
    /// # Safety
    /// The pointer must have been allocated by FFIBufManager::alloc.
    pub unsafe fn consume(&mut self, ptr: FFIBufPtr) -> FFIBuf {
        let len = self.bufs.remove(&ptr).unwrap();
        Vec::from_raw_parts(ptr, len as usize, len as usize)
    }

    pub fn encode<T: Serialize>(&mut self, data: &T) -> Result<FFIBufPtr, bincode::Error> {
        let mut buf = bincode::serialize(data)?;
        let ptr = buf.as_mut_ptr();
        self.bufs.insert(ptr, buf.len() as FFIBufLen);
        std::mem::forget(buf);
        Ok(ptr)
    }

    /// decode will consume the raw memory pointed to by ptr and return a deserialized object.
    /// After calling decode, manually deallocating the ptr is no longer needed.
    ///
    /// # Errors
    ///
    /// This function will return an error if deserialization fails. If this
    /// happens the memory pointed to by the ptr will also be dropped.
    ///
    /// # Safety
    /// The pointer must have been allocated by FFIBufManager::alloc.
    pub unsafe fn decode<T: DeserializeOwned>(
        &mut self,
        ptr: FFIBufPtr,
    ) -> Result<T, bincode::Error> {
        let buf = self.consume(ptr);
        bincode::deserialize(&buf)
    }
}

#[no_mangle]
pub fn ffi_buf_allocate(length: FFIBufLen) -> FFIBufPtr {
    fbm().alloc(length)
}

/// ffi_buf_deallocate will immediately drop the buffer pointed to by the pointer, freeing the memory
///
/// # Safety
/// The pointer must have been allocated by ffi_buf_allocate or FFIBufManager::alloc.
#[no_mangle]
pub unsafe fn ffi_buf_deallocate(ptr: FFIBufPtr) {
    fbm().dealloc(ptr)
}

#[no_mangle]
pub fn ffi_buf_len(ptr: FFIBufPtr) -> FFIBufLen {
    fbm().length(ptr)
}

extern "C" {
    fn host_log(log_req: FFIBufPtr);
}

pub struct FFILogger;

impl FFILogger {
    pub fn init(&'static self, max_level: log::Level) -> Result<(), log::SetLoggerError> {
        log::set_logger(self).map(|_| log::set_max_level(max_level.to_level_filter()))
    }
}

impl log::Log for FFILogger {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        metadata.level() <= log::max_level()
    }

    fn log(&self, record: &log::Record) {
        let record: LogRecord = record.into();
        let record_ptr = fbm().encode(&record).unwrap();
        unsafe { host_log(record_ptr) }
    }

    fn flush(&self) {
        // noop
    }
}

pub fn install_panic_hook() {
    static SET_PANIC_HOOK: Once = Once::new();
    SET_PANIC_HOOK.call_once(|| {
        std::panic::set_hook(Box::new(panic_hook));
    });
}

fn panic_hook(info: &panic::PanicInfo) {
    let record: LogRecord = info.into();
    let record_ptr = fbm().encode(&record).unwrap();
    unsafe { host_log(record_ptr) }
}
