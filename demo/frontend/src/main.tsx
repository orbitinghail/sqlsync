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

const sqlsync = await init(workerUrl, wasmUrl);

await sqlsync.open(1, 1, DEMO_REDUCER_URL);

console.log(await sqlsync.query(1, "SELECT 'bye'", []));

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>
);
