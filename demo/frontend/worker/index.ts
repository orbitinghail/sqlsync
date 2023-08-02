import * as sqlsync from "sqlsync-wasm";

const DOC = sqlsync.open(1, 1);
DOC.hello_world();

postMessage("hello");

const opfsRoot = await navigator.storage.getDirectory();

const fileHandle = await opfsRoot.getFileHandle("test.txt", { create: true });
const syncAccessHandle = await fileHandle.createSyncAccessHandle();

const textEncoder = new TextEncoder();
const content = textEncoder.encode("Hello World");
syncAccessHandle.write(content, { at: syncAccessHandle.getSize() });
syncAccessHandle.flush();

const size = syncAccessHandle.getSize();
const dataView = new DataView(new ArrayBuffer(size));

syncAccessHandle.read(dataView, { at: 0 });
console.log(new TextDecoder().decode(dataView));

syncAccessHandle.close();
