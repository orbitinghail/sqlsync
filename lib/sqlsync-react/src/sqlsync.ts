import {
  ChangedResponse,
  ErrorResponse,
  JournalId,
  OpenResponse,
  Row,
  SqlSyncRequest,
  SqlSyncResponse,
  SqlValue,
  Tags,
} from "@orbitinghail/sqlsync-worker/api.ts";

const requestId = (() => {
  let requestId = 0;
  return () => requestId++;
})();

const UTF8Encoder = new TextEncoder();

export class SqlSync {
  private port: MessagePort;
  private pending: Map<number, (msg: any) => void> = new Map();
  private onChangeTarget: EventTarget;

  constructor(port: MessagePort) {
    this.pending = new Map();
    this.port = port;
    this.port.onmessage = this.onmessage.bind(this);
    this.onChangeTarget = new EventTarget();
  }

  subscribeChanges(docId: JournalId, cb: () => void): () => void {
    this.onChangeTarget.addEventListener(docId, cb);
    return () => {
      this.onChangeTarget.removeEventListener(docId, cb);
    };
  }

  private onmessage(event: MessageEvent) {
    console.log("sqlsync: received message", event.data);

    let msg = event.data as { id: number; tag: Tags };
    let handler = this.pending.get(msg.id);
    if (handler) {
      handler(msg);
    } else if (msg.tag === "change") {
      let msg = event.data as ChangedResponse;
      this.onChangeTarget.dispatchEvent(new Event(msg.docId));
    } else {
      console.warn(`received unexpected message`, msg);
    }
  }

  private send<T extends SqlSyncRequest>(msg: T): Promise<SqlSyncResponse<T>> {
    return new Promise((resolve, reject) => {
      let id = requestId();
      console.log("sqlsync: sending message", { id, ...msg });

      this.pending.set(id, (msg: SqlSyncResponse<T> | ErrorResponse) => {
        this.pending.delete(id);
        if (msg.tag === "error") {
          reject(msg.error);
        } else {
          resolve(msg);
        }
      });

      this.port.postMessage({ id, ...msg });
    });
  }

  async boot(wasmUrl: string, coordinatorUrl?: string): Promise<void> {
    await this.send({ tag: "boot", wasmUrl, coordinatorUrl });
  }

  async open(
    docId: JournalId,
    reducerUrl: string | URL
  ): Promise<OpenResponse> {
    return await this.send({
      tag: "open",
      docId,
      reducerUrl: reducerUrl.toString(),
    });
  }

  async query<T = Row>(
    docId: JournalId,
    sql: string,
    params: SqlValue[]
  ): Promise<T[]> {
    let { rows } = await this.send({ tag: "query", docId, sql, params });
    return rows as T[];
  }

  async mutate(docId: JournalId, mutation: Uint8Array): Promise<void> {
    await this.send({ tag: "mutate", docId, mutation });
  }

  async mutateJSON(docId: JournalId, mutation: any): Promise<void> {
    const serialized = JSON.stringify(mutation);
    const bytes = UTF8Encoder.encode(serialized);
    await this.send({ tag: "mutate", docId, mutation: bytes });
  }
}

export type SqlSyncConfig = {
  workerUrl: string | URL;
  sqlSyncWasmUrl: string | URL;
  coordinatorUrl?: string | URL;
};

export default function init(config: SqlSyncConfig): Promise<SqlSync> {
  return new Promise(async (resolve) => {
    let worker = new SharedWorker(config.workerUrl, {
      type: config.workerUrl.toString().endsWith(".cjs") ? "classic" : "module",
    });
    let sqlsync = new SqlSync(worker.port);
    await sqlsync.boot(
      config.sqlSyncWasmUrl.toString(),
      config.coordinatorUrl?.toString()
    );
    console.log("sqlsync: booted worker");
    resolve(sqlsync);
  });
}