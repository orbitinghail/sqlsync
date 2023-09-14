import React from "react";
import ReactDOM from "react-dom/client";

import { RouterProvider, createBrowserRouter, redirect, useParams } from "react-router-dom";

// HACK: switch to the .ts version for nicer local dev
// import workerUrl from "@orbitinghail/sqlsync-worker/worker.ts?url";
import workerUrl from "@orbitinghail/sqlsync-worker/worker.js?url";

import sqlSyncWasmUrl from "@orbitinghail/sqlsync-worker/sqlsync.wasm?url";
import { App } from "./App";
import { journalIdFromString, journalIdToString } from "@orbitinghail/sqlsync-worker";
import { SQLSyncProvider } from "@orbitinghail/sqlsync-react";
import { MantineProvider } from "@mantine/core";

// import stylesheets
import "@mantine/core/styles.css";
import "@mantine/code-highlight/styles.css";
import { MANTINE_THEME } from "./theme";

const isLocalhost = location.hostname === "localhost" || location.hostname.startsWith("192.168");

const COORDINATOR_URL = isLocalhost
  ? `${location.hostname}:8787`
  : "sqlsync.orbitinghail.workers.dev";
const COORDINATOR_URL_WS = (isLocalhost ? "ws" : "wss") + "://" + COORDINATOR_URL;

const newDocumentId = async (name = "") => {
  let url = `${location.protocol}//${COORDINATOR_URL}/new`;
  if (name.trim().length > 0) {
    url += "/" + encodeURIComponent(name);
  }
  const response = await fetch(url, {
    method: "POST",
  });
  return journalIdFromString(await response.text());
};

export const DocRoute = () => {
  const { docId } = useParams();

  if (!docId) {
    console.error("doc id not found in params");
    return <pre style={{ color: "red" }}>ERROR: doc id not found in params</pre>;
  } else {
    return <App docId={journalIdFromString(docId)} />;
  }
};

const router = createBrowserRouter([
  {
    path: "/",
    loader: async () => {
      const docId = await newDocumentId();
      return redirect("/" + journalIdToString(docId));
    },
  },
  {
    path: "/named/:name",
    loader: async ({ params }) => {
      const docId = await newDocumentId(params.name);
      return redirect("/" + journalIdToString(docId));
    },
  },
  {
    path: "/:docId",
    element: <DocRoute />,
  },
]);

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <MantineProvider theme={MANTINE_THEME}>
      <SQLSyncProvider
        wasmUrl={sqlSyncWasmUrl}
        workerUrl={workerUrl}
        coordinatorUrl={COORDINATOR_URL_WS}
      >
        <RouterProvider router={router} />
      </SQLSyncProvider>
    </MantineProvider>
  </React.StrictMode>
);
