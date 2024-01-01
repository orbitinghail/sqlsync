import { createSignal } from "solid-js";
import { SQLSyncContext, SQLSyncProvider, createSqlSync } from "./context";
import { createDocHooks, useConnectionStatus, useSQLSync, useSqlContext } from "./hooks";
import { sql } from "./sql";
import { DocType, Row, SQLSync } from "./sqlsync";
import { serializeMutationAsJSON } from "./util";

export {
  SQLSync,
  SQLSyncContext,
  SQLSyncProvider,
  createDocHooks,
  createSignal,
  createSqlSync,
  serializeMutationAsJSON,
  sql,
  useConnectionStatus,
  useSQLSync,
  useSqlContext,
};
export type { DocType, Row };

// eof: this file only exports
