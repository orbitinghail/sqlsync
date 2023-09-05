import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App.tsx";
import "./index.css";

import {
  DocumentProvider,
  JournalId,
  SqlSyncProvider,
} from "@orbitinghail/sqlsync-react/sqlsync-react.tsx";

import sqlSyncWasmUrl from "@orbitinghail/sqlsync-worker/sqlsync.wasm?url";
import workerUrl from "@orbitinghail/sqlsync-worker/worker.ts?url";
import { Mutation } from "./mutation.ts";

const DEMO_REDUCER_URL = new URL(
  "../../../target/wasm32-unknown-unknown/release/demo_reducer.wasm",
  import.meta.url
);

const COORDINATOR_URL = "localhost:8787";
// const COORDINATOR_URL = "sqlsync.orbitinghail.workers.dev";

const newDocument = async () => {
  const response = await fetch(`${location.protocol}//${COORDINATOR_URL}/new`, {
    method: "POST",
  });
  return (await response.text()) as JournalId;
};

// check if we have a document id in the url (stored in the fragment)
const url = new URL(location.href);
const DOC_ID = (url.hash.slice(1) as JournalId) || (await newDocument());

// update the url
url.hash = DOC_ID;
history.replaceState({}, "", url.toString());

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <SqlSyncProvider
      config={{ workerUrl, sqlSyncWasmUrl, coordinatorUrl: COORDINATOR_URL }}
    >
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
