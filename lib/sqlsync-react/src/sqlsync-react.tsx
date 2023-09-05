import { JournalId, Row, SqlValue } from "@orbitinghail/sqlsync-worker/api.ts";
import React, { useCallback } from "react";
import init, { SqlSync, SqlSyncConfig, SubscribeEventDetail } from "./sqlsync";

export { randomJournalId } from "@orbitinghail/sqlsync-worker/api.ts";
export type {
  JournalId,
  Row,
  SqlValue,
} from "@orbitinghail/sqlsync-worker/api.ts";
export default init;

const SqlSyncContext = React.createContext<SqlSync | null>(null);

export function SqlSyncProvider({
  children,
  config,
}: {
  children: React.ReactNode;
  config: SqlSyncConfig;
}) {
  let [sqlsync, setSqlSync] = React.useState<SqlSync | null>(null);

  React.useEffect(() => {
    init(config).then(setSqlSync);
  }, [config.workerUrl, config.sqlSyncWasmUrl]);

  return (
    <SqlSyncContext.Provider value={sqlsync}>
      {children}
    </SqlSyncContext.Provider>
  );
}

export type DocumentState = {
  docId: JournalId;
  changes: number;
  connected: boolean;
};

const DocumentContext = React.createContext<DocumentState | null>(null);

export function DocumentProvider<Mutation>({
  children,
  docId,
  reducerUrl,
  initMutation,
}: {
  children: React.ReactNode;
  initMutation?: Mutation;
  docId: JournalId;
  reducerUrl: string | URL;
}) {
  let sqlsync = React.useContext(SqlSyncContext);
  let [doc, setDoc] = React.useState<DocumentState | null>(null);

  React.useEffect(() => {
    if (!sqlsync) return;

    sqlsync.open(docId, reducerUrl).then(() => {
      // run initMutation if defined
      if (initMutation) {
        sqlsync?.mutateJSON(docId, initMutation);
      }

      return setDoc({
        docId,
        changes: 0,
        connected: false,
      });
    });

    // subscribe to doc changes
    return sqlsync?.subscribe(docId, (evt) => {
      let e = evt as CustomEvent<SubscribeEventDetail>;
      setDoc((doc) => {
        let d = doc ? doc : { docId, changes: 0, connected: false };
        if (e.detail.tag == "change") {
          return { ...d, changes: d.changes + 1 };
        } else if (e.detail.tag == "connected") {
          return { ...d, connected: e.detail.connected };
        }
        return d;
      });
    });
  }, [sqlsync, docId, reducerUrl]);

  return (
    <DocumentContext.Provider value={doc}>{children}</DocumentContext.Provider>
  );
}

// useQuery
type QueryState<T> = {
  rows: T[];
  loading: boolean;
  error: Error | null;
};

export function useQuery<T = Row>(
  query: string,
  ...params: SqlValue[]
): {
  rows: T[];
  loading: boolean;
  error: Error | null;
} {
  let sqlsync = React.useContext(SqlSyncContext);
  let doc = React.useContext(DocumentContext);

  let [state, setState] = React.useState<QueryState<T>>({
    rows: [],
    loading: true,
    error: null,
  });

  React.useEffect(() => {
    if (!sqlsync || !doc) return;

    let mounted = true;
    sqlsync
      .query<T>(doc.docId, query, params)
      .then((rows) => {
        if (!mounted) return;
        setState({ rows, loading: false, error: null });
      })
      .catch((error) => {
        if (!mounted) return;
        setState({ rows: [], loading: false, error });
      });

    return () => {
      mounted = false;
    };
  }, [sqlsync, doc, query, ...params]);

  return state;
}

// useSqlSync
export function useSqlSync<Mutation>(): {
  mutate: (mutation: Mutation) => Promise<void>;
  query: (query: string, params: SqlValue[]) => Promise<Row[]>;
  setReplicationEnabled: (enabled: boolean) => Promise<void>;
  connected: boolean;
} {
  let sqlsync = React.useContext(SqlSyncContext);
  let doc = React.useContext(DocumentContext);

  return {
    mutate: useCallback(
      (mutation: Mutation) => {
        // TODO: eventually we should subscribe to sqlsync and queue the mutation
        if (!sqlsync || !doc) return Promise.reject("not ready");
        // TODO: support a user provided serializer rather than assuming JSON
        // serialize mutaton to JSON and convert to Uint8Array
        return sqlsync.mutateJSON(doc.docId, mutation);
      },
      [sqlsync, doc]
    ),
    query: useCallback(
      (query: string, params: SqlValue[]) => {
        // TODO: eventually we should subscribe to sqlsync and queue the mutation
        if (!sqlsync || !doc) return Promise.reject("not ready");
        return sqlsync.query(doc.docId, query, params);
      },
      [sqlsync, doc]
    ),
    setReplicationEnabled: useCallback(
      async (enabled: boolean) => {
        if (!sqlsync || !doc) return Promise.reject("not ready");
        let docId = doc.docId;
        await sqlsync.setReplicationEnabled(docId, enabled);
        return new Promise((resolve) => {
          // wait for connected event
          let unsubscribe = sqlsync?.subscribe(docId, (evt) => {
            let e = evt as CustomEvent<SubscribeEventDetail>;
            if (e.detail.tag == "connected") {
              resolve();
              unsubscribe?.();
            }
          });
        });
      },
      [sqlsync, doc]
    ),
    connected: doc?.connected ?? false,
  };
}
