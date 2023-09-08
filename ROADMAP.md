# Status and Roadmap

SQLSync is not (yet) ready for production. This file will provide a high level overview of the plan to get it there. Soon this will be migrated to Github issues and projects.

### Core
  - Schema, Reducer, and Mutation migrations
  - Presence (Cursors, Connections)
  - Wasm Component based Reducer
  - Mutation failure handling
  - Opt-in consistency with mutation forwarding to the coordinator

### SQLSync Coordinator
  - Storage snapshots
  - Pluggable authentication
  - Timeline truncation
  - Document management API
  - Query API
  - Metrics and billing APIs for SQLSync cloud

### SQLSync Browser
  - Published NPM package that enables a quick start experience
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

