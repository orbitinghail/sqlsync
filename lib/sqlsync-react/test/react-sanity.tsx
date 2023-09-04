import sqlSyncWasmUrl from "@orbitinghail/sqlsync-worker/sqlsync.wasm?url";
import workerUrl from "@orbitinghail/sqlsync-worker/worker.ts?url";
import React from "react";
import ReactDOM from "react-dom/client";
import {
  DocumentProvider,
  SqlSyncProvider,
  randomJournalId,
  useQuery,
  useSqlSync,
} from "../src/sqlsync-react";

const DEMO_REDUCER_URL = new URL(
  "../../../target/wasm32-unknown-unknown/debug/demo_reducer.wasm",
  import.meta.url
);
const DOC_ID = randomJournalId();

type Mutation =
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

ReactDOM.createRoot(document.getElementById("app")!).render(
  <React.StrictMode>
    <SqlSyncProvider config={{ workerUrl, sqlSyncWasmUrl }}>
      <DocumentProvider<Mutation>
        docId={DOC_ID}
        reducerUrl={DEMO_REDUCER_URL}
        initMutation={{ tag: "InitSchema" }}
      >
        <App />
      </DocumentProvider>
    </SqlSyncProvider>
  </React.StrictMode>
);

function App() {
  const { mutate } = useSqlSync<Mutation>();

  // HACK: force invalidations to trigger re-render
  let [invalidations, setInvalidations] = React.useState(0);

  const handleIncr = React.useCallback(() => {
    mutate({ tag: "Incr", value: 1 });
    setInvalidations((i) => i + 1);
  }, [mutate]);

  const handleDecr = React.useCallback(() => {
    mutate({ tag: "Decr", value: 1 });
    setInvalidations((i) => i + 1);
  }, [mutate]);

  const { rows, loading, error } = useQuery<{ value: number }>(
    "select value, ? from counter",
    invalidations
  );

  return (
    <>
      <h1>sqlsync-react sanity test</h1>
      <p>
        This is a sanity test for sqlsync-react. It should display a counter
        that can be incremented and decremented.
      </p>
      <p>
        The counter is stored in a SQL database, and the state is managed by
        sqlsync-react.
      </p>
      <p>
        <button onClick={handleIncr}>Incr</button>
        <button onClick={handleDecr}>Decr</button>
      </p>
      <p>
        {loading ? (
          "Loading..."
        ) : error ? (
          <span style={{ color: "red" }}>{error.message}</span>
        ) : (
          rows[0]?.value.toString()
        )}
      </p>
    </>
  );
}

// console.log("sqlsync: mutating");
// await mutate({ tag: "InitSchema" });
// await mutate({ tag: "Incr", value: 1 });
// await mutate({ tag: "Incr", value: 2 });

// console.log("sqlsync: querying");
// console.log(await sqlsync.query(DOC_ID, "select * from counter", []));
