import * as sqlsync from "sqlsync-wasm";

type Mutation =
  | { type: "InitSchema" }
  | { type: "Incr"; value: number }
  | { type: "Decr"; value: number };

const DOC = sqlsync.open(1, 1, mutator);

interface Row {
  value: number;
}

DOC.mutate(NewMutation({ type: "initSql" }));

console.log(
  DOC.query<Row>("select * from counter", []).map((row) => row.value)
);

DOC.mutate(NewMutation({ type: "incr", value: 2.5 }));

console.log(
  DOC.query<Row>("select * from counter", []).map((row) => row.value)
);

DOC.mutate(NewMutation({ type: "decr", value: 1.5 }));

console.log(
  DOC.query<Row>("select * from counter", []).map((row) => row.value)
);

DOC.mutate(NewMutation({ type: "incr", value: 2043 }));

console.log(
  DOC.query<Row>("select * from counter", []).map((row) => row.value)
);

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
