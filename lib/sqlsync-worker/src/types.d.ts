import { Row, SqlValue } from "sqlsync-worker-crate";
import { JournalId } from "./JournalId";
export type { Row, SqlValue };

export type Tags = "open" | "query" | "mutate" | "error";

type Req<T extends Tags, Body> = { tag: T } & Body;
type Res<T extends Tags, Body> = { tag: T } & Body;

export type SqlSyncRequest = Boot | Open | Query | Mutate;

export type Associate<T, In, Out> = T extends In ? Out : never;

export type SqlSyncResponse<T extends SqlSyncRequest> =
  | Associate<T, Boot, BootResponse>
  | Associate<T, Open, OpenResponse>
  | Associate<T, Query, QueryResponse>
  | Associate<T, Mutate, MutateResponse>;

export type Boot = {
  tag: "boot";
  wasmUrl: string;
  coordinatorUrl?: string;
};
export type BootResponse = { tag: "booted" };

export type Open = Req<
  "open",
  { docId: JournalId; timelineId: JournalId; reducerUrl: string }
>;
export type OpenResponse = Res<"open", {}>;

export type Query = Req<
  "query",
  { docId: JournalId; sql: string; params: SqlValue[] }
>;
export type QueryResponse = Res<"query", { rows: Row[] }>;

export type Mutate = Req<"mutate", { docId: JournalId; mutation: Uint8Array }>;
export type MutateResponse = Res<"mutate", {}>;

export type ErrorResponse = Res<"error", { error: Error }>;
