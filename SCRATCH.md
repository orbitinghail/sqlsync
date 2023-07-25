# Observations
- when syncing client -> server, if we don't have the server cursor we should request it (or the server should initialize us with it)
- my solution still feels a bit fragile - it's starting to look like libsql's virtual wal might be a more robust solution as it gives me a very clean way to handle page lookups
- better id gen for layers and timelines
- should each entry in a journal track a potential range of LSNs rather than a single LSN? this may make sync after compact easier to reason about? otherwise LSNs are effectively mutable
  - or perhaps compact should always result in a new journal (easier with COW)
- determine how many journal entries to sync based on the size of each entry rather than a fixed amount

# TODO

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

# Priorities
- networking abstraction
- opfs journal
- file journal (perhaps s3 journal?)
- libwasm
- e2e demo

# Network
- abstracts protocol + encoding
- socket based abstraction designed for websockets
- shared worker can use websocket, so can own the entire sqlsync lifecycle

Currently, networking has the following flow:

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

In general, it seems that journals could be directly synchronized by the network layer without timeline/storage being directly involved? This isn't completely true since clients have to rebase after every storage sync.

That said, it's feasible that we follow the same concept for both sides of the sync.

# decisions

- the network tier can manage caching LsnRanges for every remote journal it knows about

# network flows

**connection open**
client -> server: open documents [(storage_id, storage_range, timeline_id), ...]
*establish websocket*
server -> client: timeline LsnRange (or none) for the clients timeline_id for each doc
client -> server: trigger timeline sync if needed
server -> client: trigger storage sync if needed
client: connected
server: connected

**timeline sync**
client: prepare RequestedLsnRange from cached server LsnRange
client -> server: sync_prepare
server: ensure remote.step is scheduled
server -> client: updated LsnRange

**storage sync**
server: prepare RequestedLsnRange from cached client LsnRange
server -> client: sync_prepare
client: rebase storage
client -> server: updated LsnRange

# current structure

local
  timeline
  storage
  sqlite (vfs -> *storage)
  server_timeline_range

remote
  storage
  timelines map(journal_id -> journal)
  receive_queue priority_heap({timestamp, journal_id, LsnRange})
  sqlite (vfs -> *storage)

# proposed structure

```rust
LinkManager<D>
  links: Map(link_id -> Link{
    remote_lsn_range_cache: Map(doc_id -> LsnRange)
    docs: Vec<doc_id>
  })

  docs: Map(doc_id -> D)

  /// incoming messages are handled, and then responded to
  fn handle_msg(link_id, msg) -> msg

  /// poll for pending messages
  /// check each doc for pending work essentially
  ///   for example: timeline has changed since last poll
  fn poll_msg() -> (link_id, msg)

Document<J>
  fn sync_prepare(req: RequestedLsnRange) -> JournalPartial<J::Iter>
  fn sync_receive(partial: JournalPartial<J::Iter>) -> LsnRange

ClientDocument:
  timeline: Journal
  storage: Journal
  sqlite: (vfs -> *storage)

impl Document for ClientDocument

ServerDocument:
  timelines: Map(timeline_id -> Journal)
  timeline_receive_queue priority_heap({timestamp, journal_id, LsnRange})
  storage: Journal
  sqlite: (vfs -> *storage)

impl Document for ServerDocument
```