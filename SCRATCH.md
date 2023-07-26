# TASKS (start here)
- id
  - figure out which id abstraction to use, use it
- opfs journal
- file journal (perhaps s3 journal?)
- libwasm
- e2e demo

# Observations
- when syncing client -> server, if we don't have the server cursor we should request it (or the server should initialize us with it)
- my solution still feels a bit fragile - it's starting to look like libsql's virtual wal might be a more robust solution as it gives me a very clean way to handle page lookups
- better id gen for layers and timelines
- should each entry in a journal track a potential range of LSNs rather than a single LSN? this may make sync after compact easier to reason about? otherwise LSNs are effectively mutable
  - or perhaps compact should always result in a new journal (easier with COW)
- determine how many journal entries to sync based on the size of each entry rather than a fixed amount

## JS API + SharedWorker
- sqlite + sqlsync + mutations should all run within a single (shared) web worker
- one worker for all tabs/etc associated with the same origin
- Document oriented model, create if not exists semantics
- is it one worker per doc? or one worker overall?
- does document init require net connection?
    - document id can be client generated (dups rejected on eventual sync)
    - timeline id can be client generated
    - storage id needs to be deferred (or potentially derived from doc id)
    - storage id could == document id

## Persistence (Storage)
- plumb down a persistence layer
- goal is to back it with the OPFS API (potentially falling back to IndexedDB)
- can we leverage the sqlite page cache again?
  - it appears that it works fine, the only remaining work is to optimize the file change counter logic
- can use fileHandle.createSyncAccessHandle() + minimal overhead

# Network
- abstracts protocol + encoding
- socket based abstraction designed for websockets
- shared worker can use websocket, so can own the entire sqlsync lifecycle

Networking has the following flow:

## sync timeline
triggers:
  - connect to server
  - timeline changed (client mutates journal)

sync_timeline_prepare (journal.sync_prepare)
handle_client_sync_timeline (lookup timeline, journal.sync_receive)
sync_timeline_response (caches server lsn range)

## sync storage
triggers:
  - connect to server
  - client poll interval (optional)
  - server storage changed (server mutates journal)