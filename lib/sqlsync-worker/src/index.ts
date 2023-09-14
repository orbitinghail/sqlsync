import type {
  ConnectionStatus,
  DocEvent,
  DocId,
  DocReply,
  DocRequest,
  HandlerId,
  HostToWorkerMsg,
  QueryKey,
  SqlValue,
  WorkerToHostMsg,
} from "../sqlsync-wasm/pkg/sqlsync_wasm";

export * from "./journal-id";
export type {
  HostToWorkerMsg,
  DocRequest,
  WorkerToHostMsg,
  DocReply,
  DocEvent,
  DocId,
  SqlValue,
  HandlerId,
  QueryKey,
  ConnectionStatus,
};

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
