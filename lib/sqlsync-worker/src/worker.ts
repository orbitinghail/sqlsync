import init, { open, SqlSyncDocument } from "sqlsync-worker-crate";
import { JournalId, JournalIdToBytes } from "./JournalId";
import {
  Boot,
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

type WithId<T> = T & { id: number };

let booted = false;
let coordinatorUrl: string | undefined; // set by boot
const docs = new Map<JournalId, SqlSyncDocument>();

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

async function handle_boot(msg: Boot) {
  console.log("sqlsync: initializing wasm");
  coordinatorUrl = msg.coordinatorUrl;
  await init(msg.wasmUrl);
  booted = true;
  console.log("sqlsync: wasm initialized");
}

async function handle_open(msg: Open): Promise<OpenResponse> {
  if (!docs.has(msg.docId)) {
    console.log("sqlsync: opening document", msg.docId);
    let reducerWasmBytes = await fetchBytes(msg.reducerUrl);
    let doc = open(
      JournalIdToBytes(msg.docId),
      JournalIdToBytes(msg.timelineId),
      reducerWasmBytes,
      coordinatorUrl
    );
    docs.set(msg.docId, doc);
    return { tag: "open", alreadyOpen: false };
  }

  console.log("sqlsync: document already open", msg.docId);
  return { tag: "open", alreadyOpen: true };
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
  try {
    if (!booted) {
      if (msg.tag === "boot") {
        await handle_boot(msg);
        response = { tag: "booted" };
      } else {
        response = {
          tag: "error",
          error: new Error(`received unexpected message`),
        };
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
        response = {
          tag: "error",
          error: new Error(`received unknown message`),
        };
      }
    }
  } catch (e) {
    let error = e instanceof Error ? e : new Error(`error: ${e}`);
    response = { tag: "error", error };
  }

  port.postMessage({ id: msg.id, ...response });
}
