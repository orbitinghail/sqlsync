/* eslint-disable react-refresh/only-export-components */

import React, { useEffect } from "react";
import ReactDOM from "react-dom/client";

import sqlSyncWasmUrl from "@orbitinghail/sqlsync-worker/sqlsync.wasm?url";
import workerUrl from "@orbitinghail/sqlsync-worker/worker.ts?url";
import { JournalId, journalIdFromString } from "@orbitinghail/sqlsync-worker";
import { SQLSyncProvider } from "../src/context";
import { DocType } from "../src/sqlsync";
import { createDocHooks } from "../src/hooks";
import { serializeMutationAsJSON } from "../src/util";
import { sql } from "../src/sql";

const DEMO_REDUCER_URL = new URL(
  "../../../target/wasm32-unknown-unknown/debug/sqlsync_react_test_reducer.wasm",
  import.meta.url
);

const DOC_ID = journalIdFromString("VM7fC4gKxa52pbdtrgd9G9");

type CounterOps =
  | {
      tag: "InitSchema";
    }
  | {
      tag: "Incr";
      value: number;
    }
  | {
      tag: "Decr";
      value: number;
    };

const CounterDocType: DocType<CounterOps> = {
  reducerUrl: DEMO_REDUCER_URL,
  serializeMutation: serializeMutationAsJSON,
};

const { useMutate, useQuery } = createDocHooks(CounterDocType);

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <SQLSyncProvider wasmUrl={sqlSyncWasmUrl} workerUrl={workerUrl}>
      <App docId={DOC_ID} />
    </SQLSyncProvider>
  </React.StrictMode>
);

function App({ docId }: { docId: JournalId }) {
  const mutate = useMutate(docId);

  useEffect(() => {
    mutate({ tag: "InitSchema" }).catch((err) => {
      console.error("Failed to init schema", err);
    });
  }, [mutate]);

  const handleIncr = React.useCallback(() => {
    mutate({ tag: "Incr", value: 1 }).catch((err) => {
      console.error("Failed to incr", err);
    });
  }, [mutate]);

  const handleDecr = React.useCallback(() => {
    mutate({ tag: "Decr", value: 1 }).catch((err) => {
      console.error("Failed to decr", err);
    });
  }, [mutate]);

  const query = useQuery<{ value: number }>(
    docId,
    sql`select value, 'hi', 1.23, ${"foo"} as s from counter`
  );

  return (
    <>
      <h1>sqlsync-react sanity test</h1>
      <p>
        This is a sanity test for sqlsync-react. It should display a counter that can be incremented
        and decremented.
      </p>
      <p>The counter is stored in a SQL database, and the state is managed by sqlsync-react.</p>
      <p>
        <button onClick={handleIncr}>Incr</button>
        <button onClick={handleDecr}>Decr</button>
      </p>
      {query.state === "pending" ? (
        <pre>Loading...</pre>
      ) : query.state === "error" ? (
        <pre style={{ color: "red" }}>{query.error.message}</pre>
      ) : (
        <pre>{query.rows[0]?.value.toString()}</pre>
      )}
    </>
  );
}
