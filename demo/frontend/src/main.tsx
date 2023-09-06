import React from "react";
import ReactDOM from "react-dom/client";
import "./index.css";

import {
  DocumentProvider,
  JournalId,
  SqlSyncProvider,
} from "@orbitinghail/sqlsync-react/sqlsync-react.tsx";

import {
  RouterProvider,
  createBrowserRouter,
  redirect,
  useParams,
} from "react-router-dom";

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

const newDocument = async (name = "") => {
  let url = `${location.protocol}//${COORDINATOR_URL}/new`;
  if (name.trim().length > 0) {
    url += "/" + encodeURIComponent(name);
  }
  const response = await fetch(url, {
    method: "POST",
  });
  return (await response.text()) as JournalId;
};

export const DocRoute = () => {
  const { docId } = useParams();

  if (!docId) {
    console.error("doc id not found in params");
  }

  return (
    <DocumentProvider<Mutation>
      docId={docId as JournalId}
      reducerUrl={DEMO_REDUCER_URL}
      initMutation={{ tag: "InitSchema" }}
    >
      <App />
    </DocumentProvider>
  );
};

const router = createBrowserRouter([
  {
    path: "/",
    loader: async () => {
      const docId = await newDocument();
      return redirect("/" + docId);
    },
  },
  {
    path: "/named/:name",
    loader: async ({ params }) => {
      const docId = await newDocument(params.name);
      return redirect("/" + docId);
    },
  },
  {
    path: "/:docId",
    element: <DocRoute />,
  },
]);

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <SqlSyncProvider
      config={{ workerUrl, sqlSyncWasmUrl, coordinatorUrl: COORDINATOR_URL }}
    >
      <RouterProvider router={router} />
    </SqlSyncProvider>
  </React.StrictMode>
);
