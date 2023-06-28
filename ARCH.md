```
journal<T>:
    lsn() -> u64
    append(T)
    write(&Batch<T>)
    read(lsn, max_len) -> &Batch<T>
    rollup(lsn, (Iter<T>) -> Option<T>)
    iter() -> impl DoubleEndedIterator<Item=T> // might run into lifetime issues?

client:
    client_id: id.new()
    local: Local.new(client_id, mutator)

    // run local mutations
    local.run(AddTodo)
        timeline.apply(db, AddTodo)
            journal.append(AddTodo)
            tx = db.begin()
            mutation.apply(tx, AddTodo)
            tx.commit()

    // pull pending local mutations and send to server
    local.push_mutations(network)
        timeline.sync(network)
            batch = journal.read(server_cursor, MAX_BATCH_SIZE)
            server_cursor = network.send(SyncMutations(batch))

    local.rebase(network)
        storage.sync(network)
            // receive changes made on the server since we last synced
            batch = network.send(SyncStorage(client_cursor))
            // if batch is empty, we can abort the rebase
            if batch.empty():
                abort()
            // revert all local changes to main.db
            main_db.revert()
            // update the journal which backs main.db
            client_cursor = journal.receive(batch)
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


```