import { DocType, createDocHooks, serializeMutationAsJSON } from "@orbitinghail/sqlsync-react";

const REDUCER_URL = new URL(
  "../../../target/wasm32-unknown-unknown/release/demo_reducer.wasm",
  import.meta.url,
);

// matches the Mutation type in demo/demo-reducer
export type Mutation =
  | {
      tag: "InitSchema";
    }
  | {
      tag: "CreateTask";
      id: string;
      description: string;
    }
  | {
      tag: "DeleteTask";
      id: string;
    }
  | {
      tag: "ToggleCompleted";
      id: string;
    };

export const TaskDocType: DocType<Mutation> = {
  reducerUrl: REDUCER_URL,
  serializeMutation: serializeMutationAsJSON,
};

export const { useMutate, useQuery, useSetConnectionEnabled } = createDocHooks(TaskDocType);
