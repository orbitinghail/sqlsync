import init, {
  SqlSyncDocument,
  open,
} from "../sqlsync-wasm/pkg/sqlsync_wasm.js";

import {
  Boot,
  ChangedResponse,
  ErrorResponse,
  JournalId,
  Mutate,
  MutateResponse,
  Open,
  OpenResponse,
  Query,
  QueryResponse,
  SqlSyncRequest,
  SqlSyncResponse,
  journalIdToBytes,
  randomJournalId,
} from "./api.js";

type WithId<T> = T & { id: number };

// queue of concurrent boot requests; resolve after boot completes
let booting = false;
let bootQueue: (() => void)[] = [];

let booted = false;
let coordinatorUrl: string | undefined; // set by boot
const docs = new Map<JournalId, SqlSyncDocument>();

// TODO: connections is a memory leak since there is no reliable way to detect closed ports
let connections: MessagePort[] = [];

addEventListener("connect", (e: Event) => {
  let evt = e as MessageEvent;
  let port = evt.ports[0];
  connections.push(port);

  port.addEventListener("message", (e) => handle_message(port, e.data));
  port.start();

  console.log("sqlsync: received connection from tab");
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
  if (booting) {
    // put a promise resolve in the queue
    await new Promise<void>((resolve) => bootQueue.push(resolve));
  } else {
    booting = true;
    console.log("sqlsync: initializing wasm");
    coordinatorUrl = msg.coordinatorUrl;
    await init(msg.wasmUrl);
    booted = true;
    booting = false;
    // clear boot queue
    bootQueue.forEach((resolve) => resolve());
  }
  console.log("sqlsync: wasm initialized");
}

async function handle_open(msg: Open): Promise<OpenResponse> {
  if (!docs.has(msg.docId)) {
    console.log("sqlsync: opening document", msg.docId);
    let reducerWasmBytes = await fetchBytes(msg.reducerUrl);
    let reducerDigest = new Uint8Array(
      await crypto.subtle.digest("SHA-256", reducerWasmBytes)
    );

    // TODO: use persisted timeline id when we start persisting the journal to OPFS
    const timelineId = randomJournalId();

    const eventTarget = new EventTarget();
    eventTarget.addEventListener("change", () => {
      connections.forEach((port) =>
        port.postMessage({ tag: "change", docId: msg.docId } as ChangedResponse)
      );
    });

    let doc = open(
      journalIdToBytes(msg.docId),
      journalIdToBytes(timelineId),
      reducerWasmBytes,
      reducerDigest,
      coordinatorUrl,
      eventTarget
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

  console.time("mutate");
  doc.mutate(msg.mutation);
  console.timeEnd("mutate");
  return { tag: "mutate" };
}

async function handle_message(port: MessagePort, msg: WithId<SqlSyncRequest>) {
  let response: SqlSyncResponse<SqlSyncRequest> | ErrorResponse;

  console.log("sqlsync: received message", msg);
  try {
    if (!booted) {
      if (msg.tag === "boot") {
        await handle_boot(msg);
        response = { tag: "boot" };
      } else {
        response = {
          tag: "error",
          error: new Error(`received unexpected message`),
        };
      }
    } else {
      if (msg.tag === "boot") {
        response = { tag: "boot" };
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
