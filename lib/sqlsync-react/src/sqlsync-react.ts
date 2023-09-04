import init from "./sqlsync";

export { randomJournalId } from "@orbitinghail/sqlsync-worker/api.ts";
export type {
  JournalId,
  Row,
  SqlValue,
} from "@orbitinghail/sqlsync-worker/api.ts";
export default init;
