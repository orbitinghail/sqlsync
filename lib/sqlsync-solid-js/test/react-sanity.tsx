// import React, { useEffect } from "react";

import { JournalId } from "@orbitinghail/sqlsync-worker";
import { Match, Switch, createEffect } from "solid-js";
import { createSignal } from "solid-js/types/server/reactive.js";
import { createDocHooks } from "../src/hooks";
import { sql } from "../src/sql";
import { DocType } from "../src/sqlsync";
import { serializeMutationAsJSON } from "../src/util";

const DEMO_REDUCER_URL = new URL(
  "../../../target/wasm32-unknown-unknown/debug/sqlsync_react_test_reducer.wasm",
  import.meta.url
);

// const DOC_ID = journalIdFromString("VM7fC4gKxa52pbdtrgd9G9");

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

const [counterDocType, _setCounterDocType] = createSignal(CounterDocType);

const { useMutate, useQuery } = createDocHooks(counterDocType);

// biome-ignore lint/style/noNonNullAssertion: root is defined
// ReactDOM.createRoot(document.getElementById("root")!).render(
//   <React.StrictMode>
//     <SQLSyncProvider wasmUrl={sqlSyncWasmUrl} workerUrl={workerUrl}>
//       <App docId={DOC_ID} />
//     </SQLSyncProvider>
//   </React.StrictMode>
// );

// @ts-ignore
function App({ docId }: { docId: JournalId }) {
  const mutate = useMutate(docId);

  createEffect(() => {
    mutate({ tag: "InitSchema" }).catch((err) => {
      console.error("Failed to init schema", err);
    });
  });

  const handleIncr = () => {
    mutate({ tag: "Incr", value: 1 }).catch((err) => {
      console.error("Failed to incr", err);
    });
  };

  const handleDecr = () => {
    mutate({ tag: "Decr", value: 1 }).catch((err) => {
      console.error("Failed to decr", err);
    });
  };

  const query = useQuery<{ value: number }>(
    () => docId,
    () => sql`select value, 'hi', 1.23, ${"foo"} as s from counter`
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
        <button type="button" onClick={handleIncr}>
          Incr
        </button>
        <button type="button" onClick={handleDecr}>
          Decr
        </button>
      </p>
      <Switch>
        <Match when={query().state === "pending"}>
          <pre>Loading...</pre>
        </Match>
        <Match when={query().state === "error"}>
          <pre style={{ color: "red" }}>{(query() as any).error.message}</pre>
        </Match>
        <Match when={query().state === "success"}>
          <pre>{query().rows?.[0]?.value.toString()}</pre>
        </Match>
      </Switch>
    </>
  );
}
