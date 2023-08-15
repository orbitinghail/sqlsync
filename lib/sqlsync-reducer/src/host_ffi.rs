use serde::{de::DeserializeOwned, Serialize};
use wasmi::{AsContext, AsContextMut, Instance, Memory, TypedFunc};

pub type FFIBuf = Vec<u8>;
pub type FFIBufPtr = u32;
pub type FFIBufLen = u32;

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

pub fn consume(
    exports: WasmExports,
    mut store: impl AsContextMut,
    ptr: FFIBufPtr,
) -> anyhow::Result<FFIBuf> {
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
) -> anyhow::Result<T> {
    let buf = consume(exports, &mut store, ptr)?;
    Ok(bincode::deserialize(&buf)?)
}

pub fn encode<T: Serialize>(
    exports: WasmExports,
    mut store: impl AsContextMut,
    data: T,
) -> anyhow::Result<FFIBufPtr> {
    let bytes = bincode::serialize(&data)?;
    let len = bytes.len() as FFIBufLen;
    let ptr = exports.ffi_buf_allocate.call(&mut store, len)?;
    let mem = exports.memory.data_mut(&mut store);
    mem[ptr as usize..(ptr + len) as usize].copy_from_slice(&bytes);
    Ok(ptr)
}
