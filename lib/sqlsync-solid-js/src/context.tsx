// import { ReactNode, createContext, useEffect, useState } from "react";
import {
  Accessor,
  ParentComponent,
  createContext,
  createEffect,
  createSignal,
  onCleanup,
} from "solid-js";
import { SQLSync } from "./sqlsync";

export const SQLSyncContext = createContext<[Accessor<SQLSync | null>]>([() => null]);

interface Props {
  workerUrl: string | URL;
  wasmUrl: string | URL;
  coordinatorUrl?: string | URL;
}

const createSqlSync = (props: Props): SQLSync => {
  return new SQLSync(props.workerUrl, props.wasmUrl, props.coordinatorUrl);
};

export const SQLSyncProvider: ParentComponent<Props> = (props) => {
  const [sqlSync, setSQLSync] = createSignal<SQLSync | null>(null);

  createEffect(() => {
    const sqlSync = createSqlSync(props);
    setSQLSync(sqlSync);
    onCleanup(() => {
      sqlSync.close();
    });
  });

  return <SQLSyncContext.Provider value={[sqlSync]}>{props.children}</SQLSyncContext.Provider>;
};
