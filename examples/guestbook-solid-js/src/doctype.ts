import { createDocHooks } from "@orbitinghail/sqlsync-solid-js";
import { DocType, serializeMutationAsJSON } from "@orbitinghail/sqlsync-worker";

const REDUCER_URL = new URL(
  "../../../target/wasm32-unknown-unknown/release/reducer_guestbook.wasm",
  import.meta.url,
);

// Must match the Mutation type in the Rust Reducer code
export type Mutation =
  | {
      tag: "InitSchema";
    }
  | {
      tag: "AddMessage";
      id: string;
      msg: string;
    }
  | {
      tag: "DeleteMessage";
      id: string;
    };

export const TaskDocType: DocType<Mutation> = {
  reducerUrl: REDUCER_URL,
  serializeMutation: serializeMutationAsJSON,
};

export const { useMutate, useQuery, useSetConnectionEnabled } = createDocHooks(() => TaskDocType);
