import React from "react";
import ReactDOM from "react-dom/client";
import "./index.css";

import {
  DocumentProvider,
  JournalId,
  SqlSyncProvider,
} from "@orbitinghail/sqlsync-react/sqlsync-react.tsx";

// HACK: switch to the .ts version for nicer local dev
// import workerUrl from "@orbitinghail/sqlsync-worker/worker.ts?url";
import workerUrl from "@orbitinghail/sqlsync-worker/worker.js?url";

import sqlSyncWasmUrl from "@orbitinghail/sqlsync-worker/sqlsync.wasm?url";
import App from "./App";
import { Mutation } from "./mutation";

const DEMO_REDUCER_URL = new URL(
  "../../../target/wasm32-unknown-unknown/release/demo_reducer.wasm",
  import.meta.url
);

// const COORDINATOR_URL = "localhost:8787";
const COORDINATOR_URL = "sqlsync.orbitinghail.workers.dev";

const newDocument = async () => {
  const response = await fetch(`${location.protocol}//${COORDINATOR_URL}/new`, {
    method: "POST",
  });
  return (await response.text()) as JournalId;
};

// create a component that async loads the document id first
export const Root = () => {
  const [docId, setDocId] = React.useState<JournalId | null>(null);
  React.useEffect(() => {
    // first try to get a doc ID out of the URL
    const url = new URL(location.href);
    const URL_HASH = url.hash.slice(1);
    if (URL_HASH.trim().length > 0) {
      setDocId(URL_HASH as JournalId);
    } else {
      // otherwise create a new document
      newDocument()
        .then((docId) => {
          // update the URL
          url.hash = docId;
          history.replaceState({}, "", url.toString());
          setDocId(docId);
        })
        .catch(console.error);
    }
  }, []);

  if (!docId) {
    return <div>Loading document...</div>;
  }
  return (
    <DocumentProvider<Mutation>
      docId={docId}
      reducerUrl={DEMO_REDUCER_URL}
      initMutation={{ tag: "InitSchema" }}
    >
      <App />
    </DocumentProvider>
  );
};

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <SqlSyncProvider
      config={{ workerUrl, sqlSyncWasmUrl, coordinatorUrl: COORDINATOR_URL }}
    >
      <Root />
    </SqlSyncProvider>
  </React.StrictMode>
);
