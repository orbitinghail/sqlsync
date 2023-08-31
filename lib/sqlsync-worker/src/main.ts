import { JournalId } from "./JournalId";
import { ErrorResponse, SqlSyncRequest, SqlSyncResponse } from "./types";

// need to re-create and export these here since vite-plugin-dts doesn't like
// including types.d.ts for some reason
export type SqlValue = undefined | null | boolean | number | string;
export type Row = { [key: string]: SqlValue };

const requestId = (() => {
  let requestId = 0;
  return () => requestId++;
})();

export class SqlSync {
  private port: MessagePort;
  private pending: Map<number, (msg: any) => void> = new Map();

  constructor(port: MessagePort) {
    this.pending = new Map();
    this.port = port;
    this.port.onmessage = this.onmessage.bind(this);
  }

  private onmessage(event: MessageEvent) {
    console.log("sqlsync: received message", event.data);
    let msg = event.data as { id: number };
    let handler = this.pending.get(msg.id);
    if (handler) {
      handler(msg);
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
    timelineId: JournalId,
    reducerUrl: string | URL
  ): Promise<void> {
    await this.send({
      tag: "open",
      docId,
      timelineId,
      reducerUrl: reducerUrl.toString(),
    });
  }

  async query(
    docId: JournalId,
    sql: string,
    params: SqlValue[]
  ): Promise<Row[]> {
    let { rows } = await this.send({ tag: "query", docId, sql, params });
    return rows;
  }

  async mutate(docId: JournalId, mutation: Uint8Array): Promise<void> {
    await this.send({ tag: "mutate", docId, mutation });
  }
}

export default function init(
  workerUrl: string | URL,
  sqlSyncWasmUrl: string | URL,
  coordinatorUrl?: string | URL
): Promise<SqlSync> {
  return new Promise(async (resolve) => {
    let worker = new SharedWorker(workerUrl, { type: "module" });
    let sqlsync = new SqlSync(worker.port);
    await sqlsync.boot(sqlSyncWasmUrl.toString(), coordinatorUrl?.toString());
    console.log("sqlsync: booted worker");
    resolve(sqlsync);
  });
}
