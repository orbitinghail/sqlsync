// import { ReactNode, createContext, useEffect, useState } from "react";
import { ParentComponent, createContext, createEffect, createSignal, onCleanup } from "solid-js";
import { SQLSync } from "./sqlsync";

export const SQLSyncContext = createContext<[() => SQLSync | null, (sqlSync: SQLSync) => void]>([
  () => null,
  () => {},
]);

interface Props {
  workerUrl: string | URL;
  wasmUrl: string | URL;
  coordinatorUrl?: string | URL;
}

export const createSqlSync = (props: Props): SQLSync => {
  return new SQLSync(props.workerUrl, props.wasmUrl, props.coordinatorUrl);
};

export const SQLSyncProvider: ParentComponent<Props> = (props) => {
  const [sqlSync, setSQLSync] = createSignal<SQLSync>(createSqlSync(props));

  createEffect(() => {
    const sqlSync = createSqlSync(props);
    setSQLSync(sqlSync);
    onCleanup(() => {
      sqlSync.close();
    });
  });

  return (
    <SQLSyncContext.Provider value={[sqlSync, setSQLSync]}>
      {props.children}
    </SQLSyncContext.Provider>
  );
};
