import { createContext } from "solid-js";
import { SQLSync } from "./sqlsync";

export const SQLSyncContext = createContext<[() => SQLSync, (sqlSync: SQLSync) => void]>();
