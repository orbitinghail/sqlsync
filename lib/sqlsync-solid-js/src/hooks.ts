import {
  ConnectionStatus,
  DocId,
  DocType,
  ParameterizedQuery,
  QuerySubscription,
  Row,
  SQLSync,
  normalizeQuery,
  pendingPromise,
} from "@orbitinghail/sqlsync-worker";
import { Accessor, createEffect, createSignal, onCleanup, useContext } from "solid-js";
import { SQLSyncContext } from "./context";

export function useSQLSync(): Accessor<SQLSync> {
  const [sqlSync] = useContext(SQLSyncContext);
  return () => {
    const value = sqlSync();
    if (!value) {
      throw new Error(
        "could not find sqlsync context value; please ensure the component is wrapped in a <SqlSyncProvider>"
      );
    }
    return value;
  };
}

type MutateFn<M> = (mutation: M) => Promise<void>;
type UseMutateFn<M> = (docId: DocId) => MutateFn<M>;

type UseQueryFn = <R = Row>(
  docId: Accessor<DocId>,
  query: Accessor<ParameterizedQuery | string>
) => Accessor<QueryState<R>>;

type SetConnectionEnabledFn = (enabled: boolean) => Promise<void>;
type UseSetConnectionEnabledFn = (docId: DocId) => SetConnectionEnabledFn;

export interface DocHooks<M> {
  useMutate: UseMutateFn<M>;
  useQuery: UseQueryFn;
  useSetConnectionEnabled: UseSetConnectionEnabledFn;
}

export function createDocHooks<M>(docType: Accessor<DocType<M>>): DocHooks<M> {
  const useMutate = (docId: DocId): MutateFn<M> => {
    const sqlsync = useSQLSync();
    return (mutation: M) => sqlsync().mutate(docId, docType(), mutation);
  };

  const useQueryWrapper = <R = Row>(
    docId: Accessor<DocId>,
    query: Accessor<ParameterizedQuery | string>
  ) => {
    return useQuery<M, R>(docType, docId, query);
  };

  const useSetConnectionEnabledWrapper = (docId: DocId) => {
    const sqlsync = useSQLSync();
    return (enabled: boolean) => sqlsync().setConnectionEnabled(docId, docType(), enabled);
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
  docType: Accessor<DocType<M>>,
  docId: Accessor<DocId>,
  rawQuery: Accessor<ParameterizedQuery | string>
): Accessor<QueryState<R>> {
  const sqlsync = useSQLSync();
  const [state, setState] = createSignal<QueryState<R>>({ state: "pending" });

  createEffect(() => {
    let query = normalizeQuery(rawQuery());

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

    sqlsync()
      .subscribe(docId(), docType(), query, subscription)
      .then(unsubResolve)
      .catch((err: Error) => {
        console.error("sqlsync: error subscribing", err);
        setState({ state: "error", error: err });
      });

    onCleanup(() => {
      unsubPromise
        .then((unsub) => unsub())
        .catch((err) => {
          console.error("sqlsync: error unsubscribing", err);
        });
    });
  });

  return state;
}

export const useConnectionStatus = (): Accessor<ConnectionStatus> => {
  const sqlsync = useSQLSync();
  const [status, setStatus] = createSignal<ConnectionStatus>(sqlsync().connectionStatus);
  createEffect(() => {
    const cleanup = sqlsync().addConnectionStatusListener(setStatus);
    onCleanup(cleanup);
  });
  return status;
};
