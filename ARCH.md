```
journal<T>:
    lsn() -> u64
    append(T)
    sync_prepare(cursor, max_len) -> &JournalPartial<T>
    sync_receive(&JournalPartial<T>)
    rollup(lsn, (Iter<T>) -> Option<T>)
    iter() -> impl DoubleEndedIterator<Item=T> // might run into lifetime issues?

client:
    client_id: id.new()
    local: Local.new(client_id, mutator)

    // run local mutations
    local.run(AddTodo)
        timeline.run(db, AddTodo)
            journal.append(AddTodo)
            tx = db.begin()
            mutation.apply(tx, AddTodo)
            tx.commit()

    // pull pending local mutations and send to server
    local.push_mutations(network)
        if !server_cursor:
            server_cursor = 0
        partial = timeline.sync_prepare(server_cursor)
            journal.sync_prepare(server_cursor, MAX_LEN)
        server_cursor = network.send(SyncTimeline(partial))

    local.rebase(network)
        request = timeline.sync_request()
        response = network.send(SyncStorage(request))
        if response.empty():
            return
        storage.revert()
        storage.sync_receive(response)

        timeline.rebase(db)
            // figure out how many of our mutations have been applied server side
            applied_cursor = db.query("select lsn from mutations where client_id = $client_id")
            journal.truncate_to(applied_cursor)
            for mutation in journal:
                timeline.apply(db, mutation)

server:
    remote: Remote.new(mutator)

    remote.recover():
        // load main.db checkpoint from fs
        // replay journal
        // initialize db

        // load all of the client journals, and sync their applied cursors based on the db
        applied_cursors = db.query("select client_id, lsn from mutations")
        client_journals.sync(applied_cursors)

    client_connection_handler(client_id, network)
        while true:
            msg = network.receive()
            result = match msg:
                case SyncMutations(batch):
                    handle_client_mutations(client_id, batch)
                case SyncStorage(client_cursor):
                    handle_client_sync_storage(client_id, client_cursor)
            network.send(result)

    handle_client_mutations(client_id, batch)
        journal = remote.client_journal(client_id)
        // this sync operation needs to be idempotent
        // so the batch probably needs to include the start cursor (lsn)
        // the journal on this side can merge the batch into it's state
        cursor = journal.receive(batch)
        return cursor

    handle_client_sync_storage(client_id, client_cursor)
        return remote.update_client(client_id, client_cursor)
            return storage.journal.read(client_cursor, MAX_BATCH_SIZE)

    remote.step()
        // find the earliest unapplied mutation from all client journals
        journal = next_journal()
        cursor, mutation = journal.next()

        // apply the mutation
        tx = db.begin()
        mutator.apply(tx, mutation)
        tx.exec("replace into mutations values ($journal.client_id, $cursor)")
        tx.commit()

        // durably commit
        storage.commit()
            // the storage journal backs main.db, so we just need to
            // commit here to make the latest set of page changes durable
            // and to tell the journal to start tracking a new changeset
            journal.commit()

        // send a broadcast to all connected clients announcing that there are available changes
        clients.announce_changes()


Analysis of sync logic
=======================

sync timeline from client to server
    let req = local.sync_timeline_prepare();
        get cached server cursor
        timeline.sync_prepare
            journal.sync_prepare
                panic if cursor is out of range
                return partial [cursor..max_len]

    let server_cursor = remote.handle_client_sync_timeline(local_id, req);
        find timeline
        timeline.sync_receive
            journal.sync_receive
                panic if partial does not overlap
                merge partial into self.data
                return end cursor
        add to ReceiveQueue
        return timeline's end cursor

    local.sync_timeline_response(server_cursor);
        cache server cursor to optimize next sync

step server
    pop entry from receive queue
    get timeline for entry
    timeline.apply_up_to(sqlite, entry.cursor)
        read applied_cursor from __sqlsync_timelines
        start_cursor = applied_cursor.next() or Cursor(0)
        apply mutations from start_cursor to entry.cursor
        update __sqlsync_timelines with new applied cursor
    storage.commit
        journal.append(self.pending)

sync storage from server to client
    let req = local.storage_cursor();
        storage.cursor
            journal.end.ok <- if journal is empty, this returns None

    let resp = remote.handle_client_sync_storage(req);
        map client cursor to cursor.next() or Cursor(0)
        if cursor <= storage.cursor()
            storage.sync_prepare
                journal.sync_prepare
                    panic if cursor is out of range
                    return partial [cursor..max_len]

    resp.and_then(|r| Some(local.sync_storage_receive(r)))
        .transpose()?;
        if partial is not empty
            storage.revert
                clear pending pages

            storage.sync_receive
                journal.sync_receive
                    panic if partial does not overlap
                    merge partial into self.data
                    return end cursor

            timeline.rebase
                read applied cursor from __sqlsync_timelines
                journal.remove_up_to(applied)
                    assert applied is in range
                    data.drain(..offset)
                    update range
                reapply remaining mutations


TODO & Observations
    - can we eliminate some panics through the type system?
    - remaining panics should be errors
    - cursor/lsn/offset logic feels brittle
        - at the very least it should be pushed entirely into an opaque layer which can be heavily tested
        - perhaps a custom range class
    - should timeline id actually be pushed into the journal to ensure that unrelated journals don't accidentally cross-sync
    - when syncing client -> server, if we don't have the server cursor we should request it (or the server should initialize us with it)


    

```