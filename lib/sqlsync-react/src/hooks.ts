import { ConnectionStatus, DocId } from "@orbitinghail/sqlsync-worker";
import { deepEqual } from "fast-equals";
import { useCallback, useContext, useEffect, useRef, useState } from "react";
import { SQLSyncContext } from "./context";
import { ParameterizedQuery, normalizeQuery } from "./sql";
import { DocType, QuerySubscription, Row, SQLSync } from "./sqlsync";
import { pendingPromise } from "./util";

export function useSQLSync(): SQLSync {
  const value = useContext(SQLSyncContext);
  if (import.meta.env.DEV && !value) {
    throw new Error(
      "could not find sqlsync context value; please ensure the component is wrapped in a <SqlSyncProvider>"
    );
  }
  return value!;
}

type MutateFn<M> = (mutation: M) => Promise<void>;
type UseMutateFn<M> = (docId: DocId) => MutateFn<M>;

type UseQueryFn = <R = Row>(docId: DocId, query: ParameterizedQuery | string) => QueryState<R>;

type SetConnectionEnabledFn = (enabled: boolean) => Promise<void>;
type UseSetConnectionEnabledFn = (docId: DocId) => SetConnectionEnabledFn;

export interface DocHooks<M> {
  useMutate: UseMutateFn<M>;
  useQuery: UseQueryFn;
  useSetConnectionEnabled: UseSetConnectionEnabledFn;
}

export function createDocHooks<M>(docType: DocType<M>): DocHooks<M> {
  const useMutate = (docId: DocId): MutateFn<M> => {
    const sqlsync = useSQLSync();
    return useCallback((mutation: M) => sqlsync.mutate(docId, docType, mutation), [sqlsync, docId]);
  };

  const useQueryWrapper = <R = Row>(docId: DocId, query: ParameterizedQuery | string) => {
    return useQuery<M, R>(docType, docId, query);
  };

  const useSetConnectionEnabledWrapper = (docId: DocId) => {
    const sqlsync = useSQLSync();
    return useCallback(
      (enabled: boolean) => sqlsync.setConnectionEnabled(docId, docType, enabled),
      [sqlsync, docId]
    );
  };

  return {
    useMutate,
    useQuery: useQueryWrapper,
    useSetConnectionEnabled: useSetConnectionEnabledWrapper,
  };
}

export type QueryState<R> =
  | { state: "pending"; rows?: R[] }
  | { state: "success"; rows: R[] }
  | { state: "error"; error: Error; rows?: R[] };

export function useQuery<M, R = Row>(
  docType: DocType<M>,
  docId: DocId,
  rawQuery: ParameterizedQuery | string
): QueryState<R> {
  const sqlsync = useSQLSync();
  const [state, setState] = useState<QueryState<R>>({ state: "pending" });

  // memoize query based on deep equality
  let query = normalizeQuery(rawQuery);
  const queryRef = useRef<ParameterizedQuery>(query);
  if (!deepEqual(queryRef.current, query)) {
    queryRef.current = query;
  }
  query = queryRef.current;

  useEffect(() => {
    const [unsubPromise, unsubResolve] = pendingPromise<() => void>();

    const subscription: QuerySubscription = {
      handleRows: (rows: Row[]) => setState({ state: "success", rows: rows as R[] }),
      handleErr: (err: string) =>
        setState((s) => ({
          state: "error",
          error: new Error(err),
          rows: s.rows,
        })),
    };

    sqlsync
      .subscribe(docId, docType, query, subscription)
      .then(unsubResolve)
      .catch((err: Error) => {
        console.error("sqlsync: error subscribing", err);
        setState({ state: "error", error: err });
      });

    return () => {
      unsubPromise
        .then((unsub) => unsub())
        .catch((err) => {
          console.error("sqlsync: error unsubscribing", err);
        });
    };
  }, [sqlsync, docId, docType, query]);

  return state;
}

export const useConnectionStatus = (): ConnectionStatus => {
  const sqlsync = useSQLSync();
  const [status, setStatus] = useState<ConnectionStatus>(sqlsync.connectionStatus);
  useEffect(() => sqlsync.addConnectionStatusListener(setStatus), [sqlsync]);
  return status;
};
