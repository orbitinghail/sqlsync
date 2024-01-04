import { DocId, DocRequest, HandlerId, SqlValue } from "../sqlsync-wasm/pkg/sqlsync_wasm";

export interface BootRequest {
  tag: "Boot";
  handlerId: HandlerId;
  coordinatorUrl?: string;
  wasmUrl: string;
}

export interface CloseRequest {
  tag: "Close";
  handlerId: HandlerId;
}

export type WorkerRequest =
  | {
      tag: "Doc";
      handlerId: HandlerId;
      docId: DocId;
      req: DocRequest;
    }
  | BootRequest
  | CloseRequest;

export type Row = Record<string, SqlValue>;
