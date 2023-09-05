import { base58 } from "@scure/base";
import { Row, SqlValue } from "../sqlsync-wasm/pkg/sqlsync_wasm.js";

export type { Row, SqlValue };

declare const JournalId: unique symbol;
export type JournalId = string & { _opaque: typeof JournalId };

export const randomJournalId = (): JournalId => {
  let bytes = crypto.getRandomValues(new Uint8Array(16));
  return journalIdFromBytes(bytes);
};

export const journalIdFromBytes = (bytes: Uint8Array): JournalId => {
  return base58.encode(bytes) as JournalId;
};

export const journalIdToBytes = (s: JournalId): Uint8Array => {
  return base58.decode(s);
};

export type Tags =
  | "boot"
  | "open"
  | "query"
  | "mutate"
  | "set-replication-enabled"
  | "change"
  | "connected"
  | "error";

type Req<T extends Tags, Body> = { tag: T } & Body;
type Res<T extends Tags, Body> = { tag: T } & Body;

export type SqlSyncRequest =
  | Boot
  | Open
  | Query
  | Mutate
  | SetReplicationEnabled;

type Associate<T, In, Out> = T extends In ? Out : never;

export type SqlSyncResponse<T extends SqlSyncRequest> =
  | Associate<T, Boot, BootResponse>
  | Associate<T, Open, OpenResponse>
  | Associate<T, Query, QueryResponse>
  | Associate<T, SetReplicationEnabled, SetReplicationEnabledResponse>
  | Associate<T, Mutate, MutateResponse>;

export type Boot = Req<"boot", { wasmUrl: string; coordinatorUrl?: string }>;
export type BootResponse = Res<"boot", {}>;

export type Open = Req<"open", { docId: JournalId; reducerUrl: string }>;
export type OpenResponse = Res<"open", { alreadyOpen: boolean }>;

export type Query = Req<
  "query",
  { docId: JournalId; sql: string; params: SqlValue[] }
>;
export type QueryResponse = Res<"query", { rows: Row[] }>;

export type Mutate = Req<"mutate", { docId: JournalId; mutation: Uint8Array }>;
export type MutateResponse = Res<"mutate", {}>;

export type SetReplicationEnabled = Req<
  "set-replication-enabled",
  { docId: JournalId; enabled: boolean }
>;
export type SetReplicationEnabledResponse = Res<"set-replication-enabled", {}>;

export type ChangedResponse = Res<"change", { docId: JournalId }>;
export type ConnectedResponse = Res<
  "connected",
  { docId: JournalId; connected: boolean }
>;

export type ErrorResponse = Res<"error", { error: Error }>;
