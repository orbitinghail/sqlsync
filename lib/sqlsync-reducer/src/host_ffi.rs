use serde::{de::DeserializeOwned, Serialize};
use thiserror::Error;
use wasmi::{
    core::{HostError, Trap},
    errors::LinkerError,
    AsContext, AsContextMut, Caller, Instance, Linker, Memory, TypedFunc,
};

use crate::types::{LogRecord, ReducerError, Requests, Responses};

pub type FFIBuf = Vec<u8>;
pub type FFIBufPtr = u32;
pub type FFIBufLen = u32;

#[derive(Debug, Copy, Clone)]
pub enum WasmFFI {
    Uninitialized,
    Initialized {
        memory: Memory,
        ffi_buf_allocate: TypedFunc<FFIBufLen, FFIBufPtr>,
        ffi_buf_deallocate: TypedFunc<FFIBufPtr, ()>,
        ffi_buf_len: TypedFunc<FFIBufPtr, FFIBufLen>,
        ffi_init_reducer: TypedFunc<(), ()>,
        ffi_reduce: TypedFunc<FFIBufPtr, FFIBufPtr>,
        ffi_reactor_step: TypedFunc<FFIBufPtr, FFIBufPtr>,
    },
}

impl WasmFFI {
    pub fn uninitialized() -> Self {
        Self::Uninitialized
    }

    pub fn initialized(store: &impl AsContext, instance: &Instance) -> Result<Self, WasmFFIError> {
        let memory = instance
            .get_memory(store, "memory")
            .ok_or(WasmFFIError::MemoryNotFound)?;
        let ffi_buf_allocate =
            instance.get_typed_func::<FFIBufLen, FFIBufPtr>(store, "ffi_buf_allocate")?;
        let ffi_buf_deallocate =
            instance.get_typed_func::<FFIBufPtr, ()>(store, "ffi_buf_deallocate")?;
        let ffi_buf_len = instance.get_typed_func::<FFIBufPtr, FFIBufLen>(store, "ffi_buf_len")?;
        let ffi_init_reducer = instance.get_typed_func::<(), ()>(store, "ffi_init_reducer")?;
        let ffi_reduce = instance.get_typed_func::<FFIBufPtr, FFIBufPtr>(store, "ffi_reduce")?;
        let ffi_reactor_step =
            instance.get_typed_func::<FFIBufPtr, FFIBufPtr>(store, "ffi_reactor_step")?;

        Ok(Self::Initialized {
            memory,
            ffi_buf_allocate,
            ffi_buf_deallocate,
            ffi_buf_len,
            ffi_init_reducer,
            ffi_reduce,
            ffi_reactor_step,
        })
    }

    fn consume(
        &self,
        mut store: impl AsContextMut,
        ptr: FFIBufPtr,
    ) -> Result<FFIBuf, WasmFFIError> {
        match self {
            Self::Uninitialized => Err(WasmFFIError::Uninitialized),
            Self::Initialized { memory, ffi_buf_deallocate, ffi_buf_len, .. } => {
                let len = ffi_buf_len.call(&mut store, ptr)?;
                let mem = memory.data(&store);
                let buf = mem[ptr as usize..(ptr + len) as usize].to_vec();
                ffi_buf_deallocate.call(&mut store, ptr)?;
                Ok(buf)
            }
        }
    }

    fn persist(&self, mut store: impl AsContextMut, buf: &[u8]) -> Result<FFIBufPtr, WasmFFIError> {
        match self {
            Self::Uninitialized => Err(WasmFFIError::Uninitialized),
            Self::Initialized { memory, ffi_buf_allocate, .. } => {
                let len = buf.len() as FFIBufLen;
                let ptr = ffi_buf_allocate.call(&mut store, len)?;
                let mem = memory.data_mut(&mut store);
                mem[ptr as usize..(ptr + len) as usize].copy_from_slice(buf);
                Ok(ptr)
            }
        }
    }

    fn decode<T: DeserializeOwned>(
        &self,
        mut store: impl AsContextMut,
        ptr: FFIBufPtr,
    ) -> Result<T, WasmFFIError> {
        let buf = self.consume(&mut store, ptr)?;
        Ok(bincode::deserialize(&buf)?)
    }

    pub fn encode<T: Serialize>(
        &self,
        mut store: impl AsContextMut,
        data: T,
    ) -> Result<FFIBufPtr, WasmFFIError> {
        let bytes = bincode::serialize(&data)?;
        self.persist(&mut store, &bytes)
    }

    pub fn init_reducer(&self, mut ctx: impl AsContextMut) -> Result<(), WasmFFIError> {
        match self {
            Self::Uninitialized => Err(WasmFFIError::Uninitialized),
            Self::Initialized { ffi_init_reducer, .. } => Ok(ffi_init_reducer.call(&mut ctx, ())?),
        }
    }

    pub fn reduce(
        &self,
        mut ctx: impl AsContextMut,
        mutation: &[u8],
    ) -> Result<Requests, WasmFFIError> {
        match self {
            Self::Uninitialized => Err(WasmFFIError::Uninitialized),
            Self::Initialized { ffi_reduce, .. } => {
                let mutation_ptr = self.persist(&mut ctx, mutation)?;
                let requests_ptr = ffi_reduce.call(&mut ctx, mutation_ptr)?;
                let requests: Result<Requests, ReducerError> =
                    self.decode(&mut ctx, requests_ptr)?;
                Ok(requests?)
            }
        }
    }

    pub fn reactor_step(
        &self,
        mut ctx: impl AsContextMut,
        responses: Responses,
    ) -> Result<Requests, WasmFFIError> {
        match self {
            Self::Uninitialized => Err(WasmFFIError::Uninitialized),
            Self::Initialized { ffi_reactor_step, .. } => {
                let responses_ptr = self.encode(&mut ctx, responses)?;
                let requests_ptr = ffi_reactor_step.call(&mut ctx, responses_ptr)?;
                let requests: Result<Requests, ReducerError> =
                    self.decode(&mut ctx, requests_ptr)?;
                Ok(requests?)
            }
        }
    }
}

#[derive(Error, Debug)]
pub enum WasmFFIError {
    #[error("Bincode Error: {0}")]
    BincodeError(#[from] bincode::Error),

    #[error("WasmError: {0}")]
    WasmError(#[from] wasmi::Error),

    #[error("ReducerError: {0}")]
    ReducerError(ReducerError),

    #[error("No export named `memory` found in Wasm instance")]
    MemoryNotFound,

    #[error("Wasm FFI must be initialized before use")]
    Uninitialized,
}

impl HostError for WasmFFIError {}

impl From<ReducerError> for WasmFFIError {
    fn from(value: ReducerError) -> Self {
        WasmFFIError::ReducerError(value)
    }
}

impl From<Trap> for WasmFFIError {
    fn from(value: Trap) -> Self {
        WasmFFIError::WasmError(value.into())
    }
}

pub fn register_log_handler(linker: &mut Linker<WasmFFI>) -> Result<(), LinkerError> {
    linker.func_wrap(
        "env",
        "host_log",
        |mut ctx: Caller<'_, WasmFFI>, record_ptr: FFIBufPtr| {
            let exports = *ctx.data();
            let record: LogRecord = exports.decode(&mut ctx, record_ptr)?;
            record.log();
            Ok(())
        },
    )?;
    Ok(())
}
