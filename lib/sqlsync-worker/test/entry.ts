import init from "../src/main.js";

const TEST_REDUCER_URL = new URL(
  "../../../target/wasm32-unknown-unknown/debug/test_reducer.wasm",
  import.meta.url
);

const SQLSYNC_WORKER_URL = new URL("../src/worker.ts", import.meta.url);
const SQLSYNC_WASM_URL = new URL(
  "/assets/sqlsync_worker_crate_bg.wasm",
  import.meta.url
);

let sqlsync = await init(SQLSYNC_WORKER_URL, SQLSYNC_WASM_URL);

console.log("sqlsync: opening document");
await sqlsync.open(1, 1, TEST_REDUCER_URL.toString());

console.log("sqlsync: querying");
console.log(await sqlsync.query(1, "select 1", []));

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
  return sqlsync.mutate(1, bytes);
};

console.log("sqlsync: mutating");
await mutate({ tag: "InitSchema" });
await mutate({ tag: "Incr", value: 1 });
await mutate({ tag: "Incr", value: 2 });

console.log("sqlsync: querying");
console.log(await sqlsync.query(1, "select * from counter", []));
