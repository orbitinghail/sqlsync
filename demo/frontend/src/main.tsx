import React from "react";
import ReactDOM from "react-dom/client";

import {
  RouterProvider,
  createBrowserRouter,
  redirect,
  useParams,
  useRouteError,
} from "react-router-dom";

import sqlSyncWasmUrl from "@orbitinghail/sqlsync-worker/sqlsync.wasm?url";
import workerUrl from "@orbitinghail/sqlsync-worker/worker.ts?worker&url";

import { Alert, Container, MantineProvider, Stack } from "@mantine/core";
import { SQLSyncProvider } from "@orbitinghail/sqlsync-react";
import { journalIdFromString, journalIdToString } from "@orbitinghail/sqlsync-worker";
import { App } from "./App";

import "@mantine/code-highlight/styles.css";
// import stylesheets
import "@mantine/core/styles.css";
import { IconInfoCircle } from "@tabler/icons-react";
import { Header } from "./components/Header";
import { MANTINE_THEME } from "./theme";

const isLocalhost = location.hostname === "localhost" || location.hostname.startsWith("192.168");

const COORDINATOR_URL = isLocalhost
  ? `${location.hostname}:8787`
  : "sqlsync.orbitinghail.workers.dev";
const COORDINATOR_URL_WS = `${isLocalhost ? "ws" : "wss"}://${COORDINATOR_URL}`;

const newDocumentId = async (name = "") => {
  let url = `${location.protocol}//${COORDINATOR_URL}/new`;
  if (name.trim().length > 0) {
    url += `/${encodeURIComponent(name)}`;
  }
  const response = await fetch(url, {
    method: "POST",
  });
  if (!response.ok) {
    throw new Error(`Failed to create new document: ${response.status}`);
  }
  return journalIdFromString(await response.text());
};

export const DocRoute = () => {
  const { docId } = useParams();

  if (!docId) {
    console.error("doc id not found in params");
    return <pre style={{ color: "red" }}>ERROR: doc id not found in params</pre>;
  }

  return <App docId={journalIdFromString(docId)} />;
};

const ErrorBoundary = () => {
  // biome-ignore lint/suspicious/noExplicitAny: could be thrown from anywhere
  const error = useRouteError() as any;
  console.error(error);
  return (
    <Container size="xs" py="sm">
      <Stack>
        <Header />
        <Alert variant="light" color="red" title="Error" icon={<IconInfoCircle />}>
          Failed to load document
          {Object.prototype.hasOwnProperty.call(error, "message") ? `: ${error.message}` : ""}
        </Alert>
      </Stack>
    </Container>
  );
};

const router = createBrowserRouter([
  {
    path: "/",
    errorElement: <ErrorBoundary />,
    loader: async () => {
      const docId = await newDocumentId();
      return redirect(`/${journalIdToString(docId)}`);
    },
  },
  {
    path: "/named/:name",
    errorElement: <ErrorBoundary />,
    loader: async ({ params }) => {
      const docId = await newDocumentId(params.name);
      return redirect(`/${journalIdToString(docId)}`);
    },
  },
  {
    path: "/:docId",
    element: <DocRoute />,
    errorElement: <ErrorBoundary />,
  },
]);

// biome-ignore lint/style/noNonNullAssertion: Don't change the root component name without testing the app
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
  </React.StrictMode>,
);
