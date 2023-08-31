import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App.tsx";
import "./index.css";

const DEMO_REDUCER_URL = new URL(
  "../../../target/wasm32-unknown-unknown/debug/demo_reducer.wasm",
  import.meta.url
);

import init from "sqlsync-worker";
import wasmUrl from "sqlsync-worker/sqlsync.wasm?url";
import workerUrl from "sqlsync-worker/worker.js?url";

const COORDINATOR_URL = "localhost:8787";

const DOC_ID = crypto.randomUUID();
const TIMELINE_ID = crypto.randomUUID();

const sqlsync = await init(workerUrl, wasmUrl);

await sqlsync.open(DOC_ID, TIMELINE_ID, DEMO_REDUCER_URL);

console.log(await sqlsync.query(DOC_ID, "SELECT 'bye'", []));

// TODO: Figure out how to make sure errors are propagated out of the worker

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>
);
