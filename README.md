# SQLSync

[![github actions](https://github.com/orbitinghail/sqlsync/actions/workflows/actions.yaml/badge.svg)](https://github.com/orbitinghail/sqlsync/actions)
[![Join the SQLSync Community](https://discordapp.com/api/guilds/1149205110262595634/widget.png?style=shield)][discord]

**SQLSync is a collaborative offline-first wrapper around SQLite** designed to synchronize web
application state between users, devices, and the edge.

**Example use cases**

- A web app with a structured file oriented data model like Figma. Each file could be a SQLSync
  database, enabling real-time local first collaboration and presense
- An embedded systems deployment running SQLSync on the edge with high tolerance for unreliable
  network conditions
- Enabling optimistic mutations on SQLite read replicas

**SQLSync Demo**

The best way to get a feel for how SQLSync behaves is to play with the
[Todo list demo](https://sqlsync-todo.pages.dev/). Clicking
[this link](https://sqlsync-todo.pages.dev/) will create a unique todo list and redirect you to it's
unique URL. You can then share that URL with friends or open it on multiple devices (or browsers) to
see the power of offline-first collaborative SQLite.

You can also learn more about SQLSync and it's goals by watching Carl's WasmCon 2023 talk.
[The recording can be found here](https://youtu.be/oLYda9jmNpk?si=7BBBdLxEj9ZQ4OvS).

**Features**

- Eventually consistent SQLite
- Optimistic reads and writes
- Real-time collaboration
- Offline-first
- Cross-tab sync
- React library

If you are interested in using or contributing to SQLSync, please [join the Discord
community][discord] and let us know what you want to build. We are excited to collaborate with you!

## Getting Started

Buckle up, this is all pretty rough at this point, but hopefully will result in SQLSync running
locally.

### Dependencies

- [Just](https://github.com/casey/just)
- [Rust](https://www.rust-lang.org/)
- [wasm-pack](https://rustwasm.github.io/wasm-pack/)
- [node.js](https://nodejs.org/en)
- [pnpm](https://pnpm.io/)

### Build Wasm artifacts

```bash
just run-with-prefix 'wasm-'
just wasm-demo-reducer --release
just package-sqlsync-worker dev
```

### Local Coordinator

> [!WARNING] Currently this seems to require modifying the wrangler.toml config file to point at
> your own Cloudflare buckets (even though they aren't being used). Work is underway to replace the
> local coordinator with a wrangler agnostic alternative optimized for local development.

```bash
cd demo/cloudflare-backend
pnpm i
pnpm dev

# then in another shell
just upload-demo-reducer release local
```

### Local Todo Demo

```bash
cd demo/frontend
pnpm i
pnpm dev
```

Then go to http://localhost:5173

### Run some tests

These tests are useful for learning more about how SQLSync works.

```bash
just unit-test
just test-end-to-end-local
just test-end-to-end-local-net
```

## Community & Contributing

If you are interested in contributing to SQLSync, please [join the Discord community][discord] and
let us know what you want to build. All contributions will be held to a high standard, and are more
likely to be accepted if they are tied to an existing task and agreed upon specification.

[![Join the SQLSync Community](https://discordapp.com/api/guilds/1149205110262595634/widget.png?style=banner2)][discord]

[discord]: https://discord.gg/etFk2N9nzC
