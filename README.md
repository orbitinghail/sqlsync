# SQLSync
![Join the SQLSync Community](https://discordapp.com/api/guilds/1149205110262595634/widget.png?style=shield)

**SQLSync is a collaborative offline-first wrapper around SQLite.** It is designed to synchronize web application state between users, devices, and the edge.

**Features**
  - Eventually consistent SQLite
  - Optimistic reads and writes
  - Real-time collaboration
  - Offline-first
  - Cross-tab sync
  - React library


## Status and Roadmap

SQLSync is not (yet) ready for production. This section will provide a high level overview of the plan to get it there.

### Core
  - Schema, Reducer, and Mutation migrations
  - Presence (Cursors, Connections)
  - Wasm Component based Reducer
  - Mutation failure handling

### SQLSync Coordinator
  - Storage snapshots
  - Pluggable authentication
  - Timeline truncation
  - Document management API
  - Query API

### SQLSync Browser
  - Local storage (OPFS & IndexedDB)
  - Connection management & status
  - Granular query subscriptions
  - Rebase performance optimization
  - Mature React, Next.js, Vue, Svelte, Angular libraries

### SQLSync Library
  - Embed friendly library for non-js apps

### Dev UX
  - Language support for Reducers
  - Coordinator dev server

## Getting Started

Buckle up, this is all pretty rough at this point, but hopefully will result in SQLSync running locally.

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
just end-to-end-local
just end-to-end-local-net
```

## Contributing

If you are interested in contributing to SQLSync, please [join the Discord community][discord] and let us know what you want to build. All contributions will be held to a high standard, and are more likely to be accepted if they are tied to an existing task and agreed upon specification.

![Join the SQLSync Community](https://discordapp.com/api/guilds/1149205110262595634/widget.png?style=banner2)

[discord]: https://discord.gg/etFk2N9nzC