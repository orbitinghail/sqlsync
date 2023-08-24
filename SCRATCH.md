# TASKS (start here)
- id
  - figure out which id abstraction to use, use it
- opfs journal
- s3 or durable functions journal
- log compaction

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

# Storage Replication Frames

Currently we send a fixed number of journal entries during each sync. This is
probably fine for timelines, but not great for storage.

It would be better to dynamically calculate the number of entries to sync based on some "weight" measurement. For example, the weight of storage entires could be the number of associated pages.