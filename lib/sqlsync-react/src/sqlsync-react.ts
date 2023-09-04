import init from "./sqlsync";

export { randomJournalId } from "sqlsync-worker/api.ts";
export type { JournalId, Row, SqlValue } from "sqlsync-worker/api.ts";
export default init;
