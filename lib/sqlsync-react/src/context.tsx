import { SQLSync } from "@orbitinghail/sqlsync-worker";
import { ReactNode, createContext, createElement, useEffect, useState } from "react";

export const SQLSyncContext = createContext<SQLSync | null>(null);

interface Props {
  children: ReactNode;
  workerUrl: string | URL;
  wasmUrl: string | URL;
  coordinatorUrl?: string | URL;
}

export const SQLSyncProvider = (props: Props) => {
  const { children, workerUrl, wasmUrl, coordinatorUrl } = props;
  const [sqlsync, setSQLSync] = useState<SQLSync | null>(null);

  useEffect(() => {
    const sqlsync = new SQLSync(workerUrl, wasmUrl, coordinatorUrl);
    setSQLSync(sqlsync);
    return () => {
      sqlsync.close();
    };
  }, [workerUrl, wasmUrl, coordinatorUrl]);

  if (sqlsync) {
    return createElement(SQLSyncContext.Provider, { value: sqlsync }, children);
  }
};
