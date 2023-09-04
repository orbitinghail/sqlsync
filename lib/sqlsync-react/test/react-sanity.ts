import wasmUrl from "@orbitinghail/sqlsync-worker/sqlsync.wasm?url";
import workerUrl from "@orbitinghail/sqlsync-worker/worker.ts?url";
import init, { randomJournalId } from "../src/sqlsync-react";

const DEMO_REDUCER_URL = new URL(
  "../../../target/wasm32-unknown-unknown/debug/demo_reducer.wasm",
  import.meta.url
);

let sqlsync = await init(workerUrl, wasmUrl);

// journal ids are either 16 or 32 bytes
let DOC_ID = randomJournalId();
let TIMELINE_ID = randomJournalId();

console.log("sqlsync: opening document");
await sqlsync.open(DOC_ID, TIMELINE_ID, DEMO_REDUCER_URL.toString());

console.log("sqlsync: querying");
console.log(await sqlsync.query(DOC_ID, "select 1", []));

type Mutation =
  | {
      tag: "InitSchema";
    }
  | {
      tag: "Incr";
      value: number;
    }
  | {
      tag: "Decr";
      value: number;
    };

const mutate = (mutation: Mutation) => {
  let buf = JSON.stringify(mutation);
  let bytes = new TextEncoder().encode(buf);
  return sqlsync.mutate(DOC_ID, bytes);
};

console.log("sqlsync: mutating");
await mutate({ tag: "InitSchema" });
await mutate({ tag: "Incr", value: 1 });
await mutate({ tag: "Incr", value: 2 });

console.log("sqlsync: querying");
console.log(await sqlsync.query(DOC_ID, "select * from counter", []));
