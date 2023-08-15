use std::{collections::BTreeMap, mem::MaybeUninit, sync::Once};

use serde::{de::DeserializeOwned, Serialize};

use crate::types::{LogParams, Query, QueryResult};

pub type FFIBuf = Vec<u8>;
pub type FFIBufPtr = *mut u8;
pub type FFIBufLen = u32;

pub struct FFIBufManager {
    // map from pointer to buffer to length of buffer
    bufs: BTreeMap<FFIBufPtr, FFIBufLen>,
}

impl FFIBufManager {
    pub fn new() -> Self {
        Self {
            bufs: BTreeMap::new(),
        }
    }

    pub fn alloc(&mut self, len: FFIBufLen) -> FFIBufPtr {
        let mut buf = Vec::with_capacity(len as usize);
        let ptr = buf.as_mut_ptr();
        self.bufs.insert(ptr, len);
        std::mem::forget(buf);
        ptr
    }

    pub fn dealloc(&mut self, ptr: FFIBufPtr) {
        self.consume(ptr);
        // immediately drops the vec, freeing the memory
    }

    pub fn length(&self, ptr: FFIBufPtr) -> FFIBufLen {
        *self.bufs.get(&ptr).unwrap()
    }

    pub fn consume(&mut self, ptr: FFIBufPtr) -> FFIBuf {
        let len = self.bufs.remove(&ptr).unwrap();
        unsafe { Vec::from_raw_parts(ptr, len as usize, len as usize) }
    }

    pub fn encode<T: Serialize>(&mut self, data: &T) -> Result<FFIBufPtr, bincode::Error> {
        let mut buf = bincode::serialize(data)?;
        let ptr = buf.as_mut_ptr();
        self.bufs.insert(ptr, buf.len() as FFIBufLen);
        std::mem::forget(buf);
        Ok(ptr)
    }

    pub fn decode<T: DeserializeOwned>(&mut self, ptr: FFIBufPtr) -> Result<T, bincode::Error> {
        let buf = self.consume(ptr);
        bincode::deserialize(&buf)
    }
}

pub fn fbm() -> &'static mut FFIBufManager {
    static mut FFI_BUF_MANAGER: MaybeUninit<FFIBufManager> = MaybeUninit::uninit();
    static ONCE: Once = Once::new();
    unsafe {
        ONCE.call_once(|| {
            let singleton = FFIBufManager::new();
            FFI_BUF_MANAGER.write(singleton);
        });
        FFI_BUF_MANAGER.assume_init_mut()
    }
}

#[no_mangle]
pub fn ffi_buf_allocate(length: FFIBufLen) -> FFIBufPtr {
    fbm().alloc(length)
}

#[no_mangle]
pub fn ffi_buf_deallocate(ptr: FFIBufPtr) {
    fbm().dealloc(ptr)
}

#[no_mangle]
pub fn ffi_buf_len(ptr: FFIBufPtr) -> FFIBufLen {
    fbm().length(ptr)
}

extern "C" {
    pub fn host_query(query: FFIBufPtr) -> FFIBufPtr;

    pub fn host_log(params: FFIBufPtr);
}

pub fn log(s: String) -> Result<(), bincode::Error> {
    let params = fbm().encode(&LogParams { message: s })?;
    unsafe { host_log(params) }
    Ok(())
}

pub fn query(req: Query) -> Result<QueryResult, bincode::Error> {
    let req_ptr = fbm().encode(&req)?;
    let res_ptr = unsafe { host_query(req_ptr) };
    fbm().decode(res_ptr)
}

#[macro_export]
macro_rules! export_reducer {
    // fn should be (Mutation) -> Result<(), ReducerError>
    ($mutation:ty, $fn:ident) => {
        #[no_mangle]
        pub fn reduce(mutation_ptr: FFIBufPtr) -> FFIBufPtr {
            use sqlsync_reducer::types::ReducerError;
            fn inner(mutation_ptr: FFIBufPtr) -> Result<(), ReducerError> {
                let mutation: $mutation = fbm().decode(mutation_ptr)?;
                $fn(mutation)
            }
            match inner(mutation_ptr) {
                Ok(()) => std::ptr::null_mut(),
                Err(e) => fbm().encode(&e).unwrap(),
            }
        }
    };
}
