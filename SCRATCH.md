# Observations
- should timeline id actually be pushed into the journal to ensure that unrelated journals don't accidentally cross-sync - i.e. make it journal id and potentially include it in `RequestedLsnRange`
- when syncing client -> server, if we don't have the server cursor we should request it (or the server should initialize us with it)
- my solution still feels a bit fragile - it's starting to look like libsql's virtual wal might be a more robust solution as it gives me a very clean way to handle page lookups
- better id gen for layers and timelines
- should each entry in a journal track a potential range of LSNs rather than a single LSN? this may make sync after compact easier to reason about? otherwise LSNs are effectively mutable
  - or perhaps compact should always result in a new journal (easier with COW)

# TODO

## JS API + SharedWorker
- sqlite + sqlsync + mutations should all run within a single (shared) web worker
- one worker for all tabs/etc associated with the same origin
- Document oriented model, create if not exists semantics
- is it one worker per doc? or one worker overall?
- does document init require net connection?

## Persistence (Storage)
- plumb down a persistence layer
- goal is to back it with the OPFS API (potentially falling back to IndexedDB)
- read/write/flush
- can we leverage the sqlite page cache again?
- can use fileHandle.createSyncAccessHandle() + minimal overhead

## Network
- abstracts protocol + encoding
- socket based abstraction designed for websockets
- shared worker can use websocket, so can own the entire sqlsync lifecycle

# Persistence

The plan is to persist at the journal layer.

Currently we store Mutations and SparsePages objects in the Journal.

Generically we need the ability to efficiently insert a range of entries at a position, possibly overwriting some existing entries. We also will periodically remove part of the journal's prefix, and eventually inject a new entry at the head of the journal (for compaction).

The journal currently only allows reads via iterator. Either iter the entire journal or iter a lsn range.

When reading mutations, we just want to read the entire mutation into memory and then pass it to Mutator.apply(). So, it's probably fine if we just deserialize from some []u8 representation.

When reading sparse pages, we currently only want to read a single page overall. This requires iterating through the journal in reverse, searching for the latest version of a page. Ideally, we don't have to materialize the entire sparse pages object into memory to do this. So this implies journal storage should support seeking within each object.

*based on the above*

It seems that Journal should be abstracted over a FS api of some kind.
It's not clear if the journal should write a file per entry or use it's own single file format.

If we want file per entry, then the api could expose FS operations:
create, remove, read, write, flush...

Another option is to expose an api that maps to a list of byte arrays.
would need remove, insert?, push, read, write?

Another option is to just expose a flat file like object
read, write, resize, flush

**something to think about**
the network layer could use the same disk format for transmission
it needs to be able to move both mutations and SparsePages objects, which if we do the above fs stuff will need to be made portable anyways

# New Journal implementation

```rust
use thiserror::Error;
use positioned_io::{ReadAt, Size}

#[derive(Error, Debug)]
pub enum JournalError {
    #[error("lsn {0} is not found")]
    LsnNotFound(Lsn)
}

type Result<T> = Result<T, JournalError>

type JournalId = u64;

trait Journal {
    type Entry: ReadAt + Size;
    type Iter: DoubleEndedIterator<Item = Result<Entry>>;

    // TODO: eventually this needs to be a UUID of some kind
    fn id(&self) -> JournalId;

    /// iterate over journal entries
    fn iter(&self) -> Result<Self::Iter>;
    fn iter_range(&self, range: Range) -> Result<Self::Iter>;

    /// append a new journal entry, and then write to it
    fn append(&mut self, writer: F) -> Result<()>
    where
        F: FnOnce(entry: io::Write) -> Result<()>;

    /// sync
    fn sync_request(max_sync: usize) -> RequestedLsnRange;
    fn sync_prepare(req: RequestedLsnRange) -> Option<JournalPartial>;
    fn sync_receive(partial: JournalPartial) -> Result<LsnRange>;

    /// drop the journal's prefix
    fn drop_prefix(up_to: Lsn) -> Result<()>;
}
```

# Storage & Timeline updates
Once this is done, Storage and Timeline will need to be updated to use this Journal concept.

## Timeline

```rust
// appending a new mutation
self.journal.append(|writer| bincode::serialize_into(writer, mutation))

// rebasing mutations
self.journal
    .iter()
    .map(|reader| bincode::deserialize_from::<_, Mutation>(reader))
```

## Storage

similar to timeline, except using a custom serialize/view wrapper around sparse pages:

```rust
struct SerializedPages<I: ReadAt + Size> { reader: I }

impl<I: ReadAt + Size> SerializedPages<I> {
    fn serialize_into(writer: io::Write, pages: SparsePages) -> Result<()>
    fn new(reader: I) -> SerializedPages<I>

    fn read(&self, page_idx: PageIdx) -> Option<&Page>
    fn max_page_idx(&self) -> Option<PageIdx>
}
```