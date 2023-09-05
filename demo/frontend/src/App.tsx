import {
  useQuery,
  useSqlSync,
} from "@orbitinghail/sqlsync-react/sqlsync-react.tsx";
import React from "react";
import Checkbox from "./Checkbox";
import { Mutation } from "./mutation";

interface Task {
  id: string;
  description: string;
  completed: boolean;
}
const Task = (props: Task) => {
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

export default function App() {
  const { mutate } = useSqlSync<Mutation>();

  const [inputValue, setInputValue] = React.useState("");

  const { rows: tasks } = useQuery<Task>("select * from tasks");

  const handleCreate = React.useCallback(async () => {
    if (!inputValue.trim()) {
      return;
    }
    const id = crypto.randomUUID();
    await mutate({ tag: "CreateTask", id, description: inputValue });
    setInputValue("");
  }, [inputValue, mutate]);

  return (
    <div className="h-100 w-full flex items-center justify-center bg-teal-lightest font-sans">
      <div className="bg-white rounded shadow p-6 m-4 w-full lg:w-3/4 lg:max-w-lg">
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
      </div>
    </div>
  );
}
