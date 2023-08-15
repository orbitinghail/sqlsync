use std::{error::Error, fmt::Display};

use serde::{de::DeserializeOwned, Serialize};
use wasmi::{
    core::{HostError, Trap},
    errors::LinkerError,
    AsContext, AsContextMut, Caller, Instance, Linker, Memory, TypedFunc,
};

use crate::types::{ExecRequest, ExecResponse, LogRequest, QueryRequest, QueryResponse};

pub type FFIBuf = Vec<u8>;
pub type FFIBufPtr = u32;
pub type FFIBufLen = u32;

#[derive(Debug)]
pub struct HostState<T> {
    pub state: T,
    wasm_exports: Option<WasmExports>,
}

impl<T> HostState<T> {
    pub fn new(state: T) -> Self {
        Self {
            state,
            wasm_exports: None,
        }
    }

    pub fn initialize(&mut self, exports: WasmExports) {
        self.wasm_exports = Some(exports)
    }

    pub fn state_ref(&self) -> &T {
        &self.state
    }

    pub fn state_mut(&mut self) -> &mut T {
        &mut self.state
    }

    pub fn exports(&self) -> WasmExports {
        self.wasm_exports.as_ref().unwrap().to_owned()
    }
}

#[derive(Debug, Copy, Clone)]
pub struct WasmExports {
    memory: Memory,
    ffi_buf_allocate: TypedFunc<FFIBufLen, FFIBufPtr>,
    ffi_buf_deallocate: TypedFunc<FFIBufPtr, ()>,
    ffi_buf_len: TypedFunc<FFIBufPtr, FFIBufLen>,
}

impl WasmExports {
    pub fn new(store: &impl AsContext, instance: &Instance) -> anyhow::Result<Self> {
        let memory = instance.get_memory(store, "memory").unwrap();
        let ffi_buf_allocate =
            instance.get_typed_func::<FFIBufLen, FFIBufPtr>(store, "ffi_buf_allocate")?;
        let ffi_buf_deallocate = instance.get_typed_func::<u32, ()>(store, "ffi_buf_deallocate")?;
        let ffi_buf_len = instance.get_typed_func::<u32, u32>(store, "ffi_buf_len")?;

        Ok(Self {
            memory,
            ffi_buf_allocate,
            ffi_buf_deallocate,
            ffi_buf_len,
        })
    }
}

pub fn register_host_fns<T>(
    linker: &mut Linker<HostState<T>>,
    log: impl Fn(&mut T, String) + std::marker::Sync + std::marker::Send + 'static,
    query: impl Fn(&mut T, QueryRequest) -> Result<QueryResponse, HostFFIError>
        + std::marker::Sync
        + std::marker::Send
        + 'static,
    execute: impl Fn(&mut T, ExecRequest) -> Result<ExecResponse, HostFFIError>
        + std::marker::Sync
        + std::marker::Send
        + 'static,
) -> Result<(), LinkerError> {
    linker.func_wrap(
        "env",
        "host_log",
        move |mut caller: Caller<'_, HostState<T>>, req: FFIBufPtr| -> Result<(), Trap> {
            let exports = caller.data().exports();
            let params: LogRequest = decode(exports, &mut caller, req)?;
            let state = caller.data_mut().state_mut();
            log(state, params.message);
            Ok(())
        },
    )?;

    linker.func_wrap(
        "env",
        "host_query",
        move |mut caller: Caller<'_, HostState<T>>, req: FFIBufPtr| -> Result<FFIBufPtr, Trap> {
            let exports = caller.data().exports();
            let params: QueryRequest = decode(exports, &mut caller, req)?;
            let state = caller.data_mut().state_mut();
            let res = query(state, params)?;
            Ok(encode(exports, &mut caller, res)?)
        },
    )?;

    linker.func_wrap(
        "env",
        "host_execute",
        move |mut caller: Caller<'_, HostState<T>>, req: FFIBufPtr| -> Result<FFIBufPtr, Trap> {
            let exports = caller.data().exports();
            let params: ExecRequest = decode(exports, &mut caller, req)?;
            let state = caller.data_mut().state_mut();
            let res = execute(state, params)?;
            Ok(encode(exports, &mut caller, res)?)
        },
    )?;

    Ok(())
}

pub fn consume(
    exports: WasmExports,
    mut store: impl AsContextMut,
    ptr: FFIBufPtr,
) -> Result<FFIBuf, HostFFIError> {
    let len = exports.ffi_buf_len.call(&mut store, ptr)?;
    let mem = exports.memory.data(&store);
    let buf = mem[ptr as usize..(ptr + len) as usize].to_vec();
    exports.ffi_buf_deallocate.call(&mut store, ptr)?;
    Ok(buf)
}

pub fn decode<T: DeserializeOwned>(
    exports: WasmExports,
    mut store: impl AsContextMut,
    ptr: FFIBufPtr,
) -> Result<T, HostFFIError> {
    let buf = consume(exports, &mut store, ptr)?;
    Ok(bincode::deserialize(&buf)?)
}

pub fn encode<T: Serialize>(
    exports: WasmExports,
    mut store: impl AsContextMut,
    data: T,
) -> Result<FFIBufPtr, HostFFIError> {
    let bytes = bincode::serialize(&data)?;
    let len = bytes.len() as FFIBufLen;
    let ptr = exports.ffi_buf_allocate.call(&mut store, len)?;
    let mem = exports.memory.data_mut(&mut store);
    mem[ptr as usize..(ptr + len) as usize].copy_from_slice(&bytes);
    Ok(ptr)
}

#[derive(Debug)]
pub enum HostFFIError {
    BincodeError(bincode::Error),
    WasmTrap(Trap),
    Other(String),
}

impl Display for HostFFIError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "HostFFIError: {:?}", self)
    }
}

impl Error for HostFFIError {}

impl HostError for HostFFIError {}

impl From<bincode::Error> for HostFFIError {
    fn from(e: bincode::Error) -> Self {
        Self::BincodeError(e)
    }
}

impl From<Trap> for HostFFIError {
    fn from(value: Trap) -> Self {
        Self::WasmTrap(value)
    }
}
