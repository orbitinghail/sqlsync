import {
  useQuery,
  useSqlSync,
} from "@orbitinghail/sqlsync-react/sqlsync-react.tsx";
import React, { useMemo } from "react";
import Checkbox from "./Checkbox";
import { Mutation } from "./mutation";

// Foldable is a component that renders a H1 header, and a button which expands to render it's children
export const Foldable = ({
  header,
  children,
}: {
  header: string;
  children: React.ReactNode;
}) => {
  const [expanded, setExpanded] = React.useState(false);
  // use a unicode caret to show expanded state
  const arrow = expanded ? "▼ " : "▶ ";
  return (
    <div className="flex flex-col mb-4 w-full">
      <h1 className="cursor-pointer" onClick={() => setExpanded(!expanded)}>
        {arrow}
        {header}
      </h1>
      {expanded && children}
    </div>
  );
};

// QueryViewer is a component which let's the user type any sql query they want,
// and it displays the output of that query or any errors.
export const QueryViewer = () => {
  const [inputValue, setInputValue] = React.useState("select * from tasks");
  const { rows, error } = useQuery(inputValue);

  const rows_string = useMemo(() => {
    return JSON.stringify(
      rows,
      (_, value) => {
        // handle bigint values
        if (typeof value === "bigint") {
          return value.toString();
        }
        // eslint-disable-next-line @typescript-eslint/no-unsafe-return
        return value;
      },
      2
    );
  }, [rows]);

  return (
    <>
      <div className="mt-4">
        <p>
          Enter any valid SQLite query into the textarea below. The only
          available table is `tasks` in this demo.
        </p>
        <textarea
          className="shadow appearance-none border rounded w-full py-2 px-3 mr-4 text-grey-darker"
          value={inputValue}
          onChange={(e) => setInputValue(e.target.value)}
        />
      </div>
      <div>
        <h2>Output</h2>
        <pre className="mt-4">{error ? error.message : rows_string}</pre>
      </div>
    </>
  );
};

interface Task {
  id: string;
  description: string;
  completed: boolean;
}
export const Task = (props: Task) => {
  const { mutate } = useSqlSync<Mutation>();

  const handleDelete = React.useCallback(async () => {
    await mutate({ tag: "DeleteTask", id: props.id });
  }, [props.id, mutate]);

  const handleToggleCompleted = React.useCallback(async () => {
    await mutate({ tag: "ToggleCompleted", id: props.id });
  }, [props.id, mutate]);

  return (
    <div className="flex mb-4 items-center">
      <div className="flex-no-shrink mr-4">
        <Checkbox checked={props.completed} onChange={handleToggleCompleted} />
      </div>
      <p className="w-full">{props.description}</p>
      <button
        onClick={handleDelete}
        className="flex-no-shrink p-2 ml-2 border-2 rounded text-red border-red hover:text-white hover:bg-red-500"
      >
        Remove
      </button>
    </div>
  );
};

export function ConnectionState() {
  const { connected, setReplicationEnabled } = useSqlSync();
  const [loading, setLoading] = React.useState(false);
  return (
    <div className="flex mb-4">
      <div className="flex-auto">
        Connection Status: {connected ? "Connected" : "Disconnected"}
      </div>
      <div className="flex-no-shrink">
        <Checkbox
          checked={connected}
          disabled={loading}
          onChange={async (e) => {
            setLoading(true);
            await setReplicationEnabled(e.target.checked);
            setLoading(false);
          }}
        />
      </div>
    </div>
  );
}

export default function App() {
  const { mutate } = useSqlSync<Mutation>();

  const [inputValue, setInputValue] = React.useState("");

  const { rows: tasks } = useQuery<Task>("select * from tasks order by id");

  const handleCreate = React.useCallback(async () => {
    if (!inputValue.trim()) {
      return;
    }
    const id = crypto.randomUUID();
    await mutate({ tag: "CreateTask", id, description: inputValue });
    setInputValue("");
  }, [inputValue, mutate]);

  return (
    <div className="h-100 flex-col w-full flex items-center justify-center font-sans">
      <div className="bg-white rounded shadow p-6 m-4 w-full lg:w-3/4 lg:max-w-lg">
        <ConnectionState />
        <div className="mb-4">
          <h1 className="text-grey-darkest">Todo List</h1>
          <div className="flex mt-4">
            <input
              className="shadow appearance-none border rounded w-full py-2 px-3 mr-4 text-grey-darker"
              placeholder="Add Todo"
              value={inputValue}
              onChange={(e) => setInputValue(e.target.value)}
              onKeyUp={async (e) => {
                if (e.key === "Enter") {
                  await handleCreate();
                }
              }}
            />
            <button
              type="submit"
              onClick={handleCreate}
              className="flex-no-shrink p-2 border-2 rounded text-teal border-teal hover:text-white hover:bg-teal-500"
            >
              Add
            </button>
          </div>
        </div>
        <div>
          {tasks.map((task) => (
            <Task key={task.id} {...task} />
          ))}
        </div>
        <Foldable header="Query Viewer">
          <QueryViewer />
        </Foldable>
      </div>
    </div>
  );
}
