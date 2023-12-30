// import { ReactNode, createContext, useEffect, useState } from "react";
import {
  ParentComponent,
  Show,
  createContext,
  createEffect,
  createSignal,
  onCleanup,
} from "solid-js";
import { SQLSync } from "./sqlsync";

export const SQLSyncContext = createContext<SQLSync | null>(null);

interface Props {
  workerUrl: string | URL;
  wasmUrl: string | URL;
  coordinatorUrl?: string | URL;
}

export const SQLSyncProvider: ParentComponent<Props> = (props) => {
  const [sqlsync, setSQLSync] = createSignal<SQLSync | null>(null);

  createEffect(() => {
    const sqlsync = new SQLSync(props.workerUrl, props.wasmUrl, props.coordinatorUrl);
    setSQLSync(sqlsync);
    onCleanup(() => {
      sqlsync.close();
    });
  });

  return (
    <Show when={sqlsync()} keyed>
      {(sqlSync) => {
        return <SQLSyncContext.Provider value={sqlSync}>{props.children}</SQLSyncContext.Provider>;
      }}
    </Show>
  );
};
