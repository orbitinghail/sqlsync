# The SQLSync Guide

> [!IMPORTANT]
> SQLSync is in active development and thus is changing quickly. Currently, do not use it in a production application as there is no backwards compatibility or stability promise.

SQLSync is distributed as a JavaScript package as well as a Rust Crate.  Currently, both are required to use SQLSync. Also, React is the only supported framework at the moment.

If you want to jump ahead to a working demo, check out the finished product at: https://github.com/orbitinghail/sqlsync-demo-guestbook

## Step 1: Creating the Reducer

SQLSync requires that all mutations are handled by a piece of code called "The Reducer". Currently, this code has to be written in Rust, however we have plans to make it possible to write Reducers using JS or other languages. The fastest way to create a reducer is to initialize a new Rust project like so:

1. Make sure you have Rust stable installed; if not install using [rustup]:

```bash
rustup toolchain install stable
rustup default stable
```

2. Install support for the `wasm32-unknown-unknown` target:

```bash
rustup target add wasm32-unknown-unknown
```

3. Initialize the reducer: (feel free to rename)

```bash
cargo init --lib reducer
cd reducer
```

4. Update `Cargo.toml` to look something like this

```toml
[package]
name = "reducer"
version = "0.1.0"
edition = "2021"

[lib]
crate-type = ["cdylib"]

[profile.release]
lto = true
strip = "debuginfo"
codegen-units = 1

[dependencies]
sqlsync-reducer = "0.1"
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
log = "0.4"
```

5. Update `src/lib.rs` to look something like this:

```rust
use serde::Deserialize;
use sqlsync_reducer::{execute, init_reducer, types::ReducerError};

#[derive(Deserialize, Debug)]
#[serde(tag = "tag")]
enum Mutation {
  InitSchema,
  AddMessage { id: String, msg: String },
}

init_reducer!(reducer);
async fn reducer(mutation: Vec<u8>) -> Result<(), ReducerError> {
  let mutation: Mutation = serde_json::from_slice(&mutation[..])?;

  match mutation {
    Mutation::InitSchema => {
      execute!(
        "CREATE TABLE IF NOT EXISTS messages (
          id TEXT PRIMARY KEY,
          msg TEXT NOT NULL,
          created_at TEXT NOT NULL
        )"
      ).await?;
    }

    Mutation::AddMessage { id, msg } => {
      log::info!("appending message({}): {}", id, msg);
      execute!(
        "insert into messages (id, msg, created_at)
          values (?, ?, datetime('now'))",
        id, msg
      ).await?;
    }
  }

  Ok(())
}
```

6. Compile your reducer to Wasm

```bash
cargo build --target wasm32-unknown-unknown --release
```

> [!IMPORTANT]
> Currently Rust nightly will fail to build reducers to Wasm. Please make sure you are using Rust stable. You can use it as a one-off with the command `rustup run stable cargo build...`

## Step 2: Install and configure the React library

```bash
npm install @orbitinghail/sqlsync-react @orbitinghail/sqlsync-worker
```

The following examples will be using Typescript to make everything a bit more precise. If you are not using Typescript you can still use SQLSync, just skip the type descriptions and annotations.

Also, make sure your JS bundling tool supports importing assets from the file system, as will need that to easily get access to the Reducer we compiled earlier in this guide. If in doubt, [Vite] is highly recommended.

Create a file which will contain type information for your Mutations, the reducer URL, and export some useful React hooks for your app to consume. It should look something like this:

```typescript
import {
  DocType,
  createDocHooks,
  serializeMutationAsJSON,
} from "@orbitinghail/sqlsync-react";

// Path to your compiled reducer artifact, your js bundler should handle making
// this a working URL that resolves during development and in production.
const REDUCER_URL = new URL(
  "../reducer/target/wasm32-unknown-unknown/release/reducer.wasm",
  import.meta.url
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
    };

export const TaskDocType: DocType<Mutation> = {
  reducerUrl: REDUCER_URL,
  serializeMutation: serializeMutationAsJSON,
};

export const { useMutate, useQuery, useSetConnectionEnabled } =
  createDocHooks(TaskDocType);
```

## Step 3: Hooking it up to your app

Using the hooks exported from the file in [Step 2](#step-2-install-and-configure-the-react-library) we can easily hook SQLSync up to our application.

Here is a complete example of a very trivial guestbook application which uses the reducer we created above.

```tsx
import React, { FormEvent, useEffect } from "react";
import ReactDOM from "react-dom/client";

// this example uses the uuid library (`npm install uuid`)
import { v4 as uuidv4 } from "uuid";

// You'll need to configure your build system to make these entrypoints
// available as urls. Vite does this automatically via the `?url` suffix.
import sqlSyncWasmUrl from "@orbitinghail/sqlsync-worker/sqlsync.wasm?url";
import workerUrl from "@orbitinghail/sqlsync-worker/worker.js?url";

// import the SQLSync provider and hooks
import { SQLSyncProvider, sql } from "@orbitinghail/sqlsync-react";
import { useMutate, useQuery } from "./doctype";

// Create a DOC_ID to use, each DOC_ID will correspond to a different SQLite
// database. We use a static doc id so we can play with cross-tab sync.
import { journalIdFromString } from "@orbitinghail/sqlsync-worker";
const DOC_ID = journalIdFromString("VM7fC4gKxa52pbdtrgd9G9");

// Configure the SQLSync provider near the top of the React tree
ReactDOM.createRoot(document.getElementById("root")!).render(
  <SQLSyncProvider wasmUrl={sqlSyncWasmUrl} workerUrl={workerUrl}>
    <App />
  </SQLSyncProvider>
);

// Use SQLSync hooks in your app
export function App() {
  // we will use the standard useState hook to handle the message input box
  const [msg, setMsg] = React.useState("");

  // create a mutate function for our document
  const mutate = useMutate(DOC_ID);

  // initialize the schema; eventually this will be handled by SQLSync automatically
  useEffect(() => {
    mutate({ tag: "InitSchema" }).catch((err) => {
      console.error("Failed to init schema", err);
    });
  }, [mutate]);

  // create a callback which knows how to trigger the add message mutation
  const handleSubmit = React.useCallback(
    (e: FormEvent<HTMLFormElement>) => {
      // Prevent the browser from reloading the page
      e.preventDefault();

      // create a unique message id
      const id = crypto.randomUUID ? crypto.randomUUID() : uuidv4();

      // don't add empty messages
      if (msg.trim() !== "") {
        mutate({ tag: "AddMessage", id, msg }).catch((err) => {
          console.error("Failed to add message", err);
        });
        // clear the message
        setMsg("");
      }
    },
    [mutate, msg]
  );

  // finally, query SQLSync for all the messages, sorted by created_at
  const { rows } = useQuery<{ id: string; msg: string }>(
    DOC_ID,
    sql`
      select id, msg from messages
      order by created_at
    `
  );

  return (
    <div>
      <h1>Guestbook:</h1>
      <ul>
        {(rows ?? []).map(({ id, msg }) => (
          <li key={id}>{msg}</li>
        ))}
      </ul>
      <h3>Leave a message:</h3>
      <form onSubmit={handleSubmit}>
        <label>
          Msg:
          <input
            type="text"
            name="msg"
            value={msg}
            onChange={(e) => setMsg(e.target.value)}
          />
        </label>
        <input type="submit" value="Submit" />
      </form>
    </div>
  );
}
```

## Step 4: Connect to the coordinator (COMING SOON)

This step still requires using SQLSync from source. For now, you'll have to follow the directions in the [Contribution Guide] to set up a Local Coordinator.

[rustup]: https://rustup.rs/
[Vite]: https://vitejs.dev/
[Contribution Guide]: ./CONTRIBUTING.md
