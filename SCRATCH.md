# TODO & Observations
- should timeline id actually be pushed into the journal to ensure that unrelated journals don't accidentally cross-sync - i.e. make it journal id and potentially include it in `RequestedLsnRange`
- when syncing client -> server, if we don't have the server cursor we should request it (or the server should initialize us with it)
- my solution still feels a bit fragile - it's starting to look like libsql's virtual wal might be a more robust solution as it gives me a very clean way to handle page lookups
- better id gen for layers and timelines