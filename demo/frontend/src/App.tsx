import {
  useQuery,
  useSqlSync,
} from "@orbitinghail/sqlsync-react/sqlsync-react.tsx";
import React from "react";
import { Mutation } from "./mutation";

function App() {
  const { mutate } = useSqlSync<Mutation>();

  const handleIncr = React.useCallback(async () => {
    await mutate({ tag: "Incr", value: 1 });
  }, [mutate]);

  const handleDecr = React.useCallback(async () => {
    await mutate({ tag: "Decr", value: 1 });
  }, [mutate]);

  const { rows, loading, error } = useQuery<{ value: number }>(
    "select value from counter"
  );
  return (
    <>
      <h1>sqlsync demo</h1>
      <p>
        This is a sanity test for sqlsync-react. It should display a counter
        that can be incremented and decremented.
      </p>
      <p>
        The counter is stored in a SQL database, and the state is managed by
        sqlsync-react.
      </p>
      <p>
        <button onClick={handleIncr}>Incr</button>
        <button onClick={handleDecr}>Decr</button>
      </p>
      <p>
        {loading ? (
          "Loading..."
        ) : error ? (
          <span style={{ color: "red" }}>{error.message}</span>
        ) : (
          rows[0]?.value.toString()
        )}
      </p>
    </>
  );
}

export default App;
