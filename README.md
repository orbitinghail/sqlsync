# SQLSync

[![github actions](https://github.com/orbitinghail/sqlsync/actions/workflows/actions.yaml/badge.svg?branch=main)](https://github.com/orbitinghail/sqlsync/actions?query=branch%3Amain)
[![Join the SQLSync Community](https://discordapp.com/api/guilds/1149205110262595634/widget.png?style=shield)][discord]

**SQLSync is a collaborative offline-first wrapper around SQLite** designed to synchronize web application state between users, devices, and the edge.

**Example use cases**

- A web app with a structured file oriented data model like Figma. Each file could be a SQLSync database, enabling real-time local first collaboration and presence
- Running SQLSync on the edge with high tolerance for unreliable network conditions
- Enabling optimistic mutations on SQLite read replicas

**SQLSync Demo**

The best way to get a feel for how SQLSync behaves is to play with the [Todo list demo][todo-demo]. Clicking [this link][todo-demo] will create a unique to-do list and redirect you to its unique URL. You can then share that URL with friends or open it on multiple devices (or browsers) to see the power of offline-first collaborative SQLite.

[todo-demo]: https://sqlsync-todo.pages.dev/

You can also learn more about SQLSync and it's goals by watching Carl's WasmCon 2023 talk. [The recording can be found here][wasmcon-talk].

[wasmcon-talk]: https://youtu.be/oLYda9jmNpk?si=7BBBdLxEj9ZQ4OvS

**Features**

- Eventually consistent SQLite
- Optimistic reads and writes
- Reactive query subscriptions
- Real-time collaboration
- Offline-first
- Cross-tab sync
- React library

If you are interested in using or contributing to SQLSync, please [join the Discord community][discord] and let us know what you want to build. We are excited to collaborate with you!

## Installation & Getting started

Please refer to [the guide](./GUIDE.md) to learn how to add SQLSync to your application.

## Tips & Tricks

### How to debug SQLSync in the browser
By default SQLSync runs in a shared web worker. This allows the database to automatically be shared between different tabs, however results in making SQLSync a bit harder to debug.

The easiest way is to use Google Chrome, and go to the special URL: [chrome://inspect/#workers](chrome://inspect/#workers). On that page you'll find a list of all the running shared workers in other tabs. Assuming another tab is running SQLSync, you'll see the shared worker listed. Click `inspect` to open up dev-tools for the worker.

### My table is missing, or multiple statements aren't executing
SQLSync uses [rusqlite] under the hood to run and query SQLite. Unfortunately, the `execute` method only supports single statements and silently ignores trailing statements. Thus, if you are using `execute!(...)` in your reducer, make sure that each call only runs a single SQL statement.

For example:
```rust
// DON'T DO THIS:
execute!("create table foo (id int); create table bar (id int);").await?;

// DO THIS:
execute!("create table foo (id int)").await?;
execute!("create table bar (id int)").await?;
```

## Community & Contributing

If you are interested in contributing to SQLSync, please [join the Discord community][discord] and let us know what you want to build. All contributions will be held to a high standard, and are more likely to be accepted if they are tied to an existing task and agreed upon specification.

[![Join the SQLSync Community](https://discordapp.com/api/guilds/1149205110262595634/widget.png?style=banner2)][discord]

[discord]: https://discord.gg/etFk2N9nzC
[rusqlite]: https://github.com/rusqlite/rusqlite
