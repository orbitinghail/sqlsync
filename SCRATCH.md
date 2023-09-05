# TASKS (start here)
- demo frontend & basic react hooks
- readme
- talk
- timeline truncation after server persistence
  - otherwise the server is storing all timelines forever in memory
- [schema migrations](#schema-migrations) should be built into the reducer
- opfs journal
- [log compaction](#log-compaction)
- replication frames hint
  - hint that more frames are coming allowing the receiver to delay sending range ACKs and rebases, this will improve replication perf through minimizing round trips
- if mutations decide to not make any changes, don't write any updates to storage

# Schema Migrations
Basic idea is to add another api to the reducer which is used to trigger migrations when documents are created or versions change.

This is more robust than the current "InitSchema" mutation which is difficult to coordinate.

# Log compaction
With the rebase sync architecture, compacting the storage log is very easy and safe. At any point the coordinator can snapshot and start a new log from the snapshot.

The snapshot algorithm can work like this to improve efficiency on the clients:
- coordinator snapshots the log at a particular LSN
- appends a "end log" message to the previous log which contains the new log id and a hash of the snapshot
- upon receiving this marker, a client can run the snapshot process locally and check that it gets the same snapshot
- if it does, the client knows it can create a new local storage log from that snapshot and continue delta replicating from the server
- if anything goes wrong, the client can drop their storage log and download the full snapshot from the server

This algorithm depends on the server continuing to serve the old log for some
period of time - this time window determines how many clients will be able to
efficiently switch over.

Since the coordinator also wants to be durable in a serverless setting, it's
likely that the old logs will already be backed up to storage. Thus the
coordinator can always serve from the last log lazily without keeping it in
memory. (at least until GC runs and kills old logs, but that should never hit
the immediately previous log)

# rebase performance
Currently we are rebasing the timeline every time we receive a frame from the server. This is needlessly expensive and can be improved.

Two perf holes:
1. the server is sending us more frames soon
2. we have a lot of pending mutations that haven't been seen by the server

2 can sometimes imply 1, however 1 can also happen if other clients are sending tons of changes.

To optimize for 1, the server can send a hint to the client with the number of outstanding frames. This allows the client to make better decisions about rebase.

To optimize for 2, we can try to optimize using the following facts
 - how many pending mutations have yet to be acknowledged by the server
 - how fast are other timelines changing (we can determine this by looking at the timelines table in the db)

We can come up with some heuristics regarding these facts to balance user experience and rebases.

---

Note, we can also optimize rebase perf through mutation batching. Currently we pass in a single mutation to the reducer at a time. This involves many round trips through wasmi which is not very fast. It would be much faster to batch mutations to the reducer allowing reducers to optimize internally. For example, merging many mutations into a single insert/update statement.