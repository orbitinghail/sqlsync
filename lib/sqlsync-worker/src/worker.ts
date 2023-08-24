import init, { open, SqlSyncDocument } from "sqlsync-worker-crate";
import {
  ErrorResponse,
  Mutate,
  MutateResponse,
  Open,
  OpenResponse,
  Query,
  QueryResponse,
  SqlSyncRequest,
  SqlSyncResponse,
} from "./types";

let booted = false;
const docs = new Map<number, SqlSyncDocument>();

addEventListener("connect", (e: Event) => {
  let evt = e as MessageEvent;
  let port = evt.ports[0];

  port.addEventListener("message", (e) => handle_message(port, e.data));
  port.start();

  console.log("sqlsync: received connection");
});

const responseToError = (r: Response): Error => {
  let msg = `${r.status}: ${r.statusText}`;
  return new Error(msg);
};

const fetchBytes = async (url: string) =>
  fetch(url)
    .then((r) => (r.ok ? r : Promise.reject(responseToError(r))))
    .then((r) => r.arrayBuffer())
    .then((b) => new Uint8Array(b));

type WithId<T> = T & { id: number };

async function handle_open(msg: Open): Promise<OpenResponse> {
  if (!docs.has(msg.docId)) {
    let reducerWasmBytes = await fetchBytes(msg.reducerUrl);
    let doc = open(msg.docId, msg.timelineId, reducerWasmBytes);
    docs.set(msg.docId, doc);
  }
  return { tag: "open" };
}

function handle_query(msg: Query): QueryResponse {
  let doc = docs.get(msg.docId);
  if (!doc) {
    throw new Error(`no document with id ${msg.docId}`);
  }

  let rows = doc.query(msg.sql, msg.params);
  return { tag: "query", rows };
}

function handle_mutate(msg: Mutate): MutateResponse {
  let doc = docs.get(msg.docId);
  if (!doc) {
    throw new Error(`no document with id ${msg.docId}`);
  }

  doc.mutate(msg.mutation);
  return { tag: "mutate" };
}

async function handle_message(port: MessagePort, msg: WithId<SqlSyncRequest>) {
  let response: SqlSyncResponse<SqlSyncRequest> | ErrorResponse;

  console.log("sqlsync: received message", msg);

  if (!booted) {
    if (msg.tag === "boot") {
      await handle_boot(msg.wasmUrl);
      response = { tag: "booted" };
    } else {
      response = { tag: "error", error: new Error(`received unknown message`) };
    }
  } else {
    if (msg.tag === "boot") {
      response = { tag: "booted" };
    } else if (msg.tag === "open") {
      response = await handle_open(msg);
    } else if (msg.tag === "query") {
      response = handle_query(msg);
    } else if (msg.tag === "mutate") {
      response = handle_mutate(msg);
    } else {
      response = { tag: "error", error: new Error(`received unknown message`) };
    }
  }

  port.postMessage({ id: msg.id, ...response });
}

async function handle_boot(wasmUrl: string) {
  console.log("sqlsync: initializing wasm");
  await init(wasmUrl);
  booted = true;
  console.log("sqlsync: wasm initialized");
}
