// import { ReactNode, createContext, useEffect, useState } from "react";
import { ParentComponent, createContext, createSignal } from "solid-js";
import { SQLSync } from "./sqlsync";

export const SQLSyncContext = createContext<[() => SQLSync, (sqlSync: SQLSync) => void]>();

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
  // console.log("sqlSync in provider:", sqlSync(), JSON.stringify(sqlSync(), null, 2));

  // const sqlSyncValue: [Accessor<SQLSync | null>] = [sqlSync];

  // createEffect(() => {
  //   const sqlSync = createSqlSync(props);
  //   console.log("sqlSync in effect:", sqlSync, JSON.stringify(sqlSync, null, 2));
  //   setSQLSync(sqlSync);
  //   onCleanup(() => {
  //     sqlSync.close();
  //   });
  // });

  return (
    <SQLSyncContext.Provider value={[sqlSync, setSQLSync]}>
      {props.children}
    </SQLSyncContext.Provider>
  );
};
