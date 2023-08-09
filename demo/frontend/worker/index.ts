import * as sqlsync from "sqlsync-wasm";

type Mutation =
  | { type: "initSql" }
  | { type: "incr"; value: number }
  | { type: "decr"; value: number };

interface Serializer {
  serialize: () => Uint8Array;
}

const Utf8Encoder = new TextEncoder();
const Utf8Decoder = new TextDecoder();

const NewMutation = (m: Mutation): Mutation & Serializer => {
  return {
    ...m,
    serialize: () => Utf8Encoder.encode(JSON.stringify(m)),
  };
};

class MutatorHandle implements sqlsync.IMutatorHandle<Mutation & Serializer> {
  apply(
    mutation: Mutation,
    execute: sqlsync.ExecuteFn,
    // eslint-disable-next-line @typescript-eslint/no-unused-vars
    query: sqlsync.QueryFn
  ): void {
    switch (mutation.type) {
      case "initSql":
        execute(
          "CREATE TABLE IF NOT EXISTS counter (idx INTEGER PRIMARY KEY, value REAL)",
          []
        );
        break;
      case "incr":
        execute(
          `
            INSERT INTO counter (idx, value) VALUES (0, ?)
            ON CONFLICT (idx) DO UPDATE SET value = value + ?
          `,
          [mutation.value, mutation.value]
        );
        break;
      case "decr":
        execute(
          `
            INSERT INTO counter (idx, value) VALUES (0, ?)
            ON CONFLICT (idx) DO UPDATE SET value = value - ?
          `,
          [mutation.value, mutation.value]
        );
        break;
    }
  }

  deserializeMutation(data: Uint8Array): Mutation & Serializer {
    const json = Utf8Decoder.decode(data);
    return NewMutation(JSON.parse(json) as Mutation);
  }
}

const mutator = new MutatorHandle();

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
