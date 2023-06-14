```
Local
  storage: physical.StorageReplica
  sqlite: sqlite.Connection
  timeline: logical.Timeline<Mutator>

  pull() -> Batch
  cursor() -> Cursor
  rebase(Changeset)

Remote
  storage: physical.Storage
  sqlite: sqlite.Connection
  replayer: logical.TimelineReplayer<Mutator>

  replay(Batch)
  maybe_checkpoint()
  diff(Cursor) -> Changeset

logical
  Mutator (type Mutation)

  Batch
    timeline_id
    last_seq
    mutations: []Mutation

  Timeline<Mutator>
    new(timeline_id)
    // run a mutation on the db and append it to the timeline
    run(db, mutation)
    // read a batch of mutations from the timeline
    read(max_len) -> Batch
    // read seq from timelines table
    // remove mutations <= seq
    // reapply remaining mutations
    rebase(db)

  TimelineReplayer<Mutator>
    // run a batch of mutations on the db
    // update the timelines table with batch.{timeline_id,last_seq}
    run(db, Batch)

physical
  SqliteShm
    Vec<Vec<u8>>

  Page: [u8; 4k]

  LayerId

  Layer
    layer_id: LayerId
    pages: Map<idx, Page>

  Cursor
    // id of the base layer
    layer_id: LayerId
    // frame index in the wal
    frame_idx: u32

  Changeset
    cursor: Cursor
    pages: Map<idx, Page>

  Storage
    layers: Vec<Layer>
    wal: Vec<u8>
    shm: SqliteShm

    maybe_checkpoint(max_pages)
      if wal.len() > max_pages:
        create new Layer
        copy from wal into Layer
        append to self.layers
        reset wal & shm

    diff(cursor) -> Changeset
      cs = Changeset.new()
      for layer in self.layers:
        if layer.id > cursor.layer_id:
          cs.copy_from(layer)
      for (idx,frame) in wal:
        if idx > cursor.frame_idx
          cs.write(idx*PAGE_SIZE, frame)

  StorageReplica
    cursor: Cursor
    main: Vec<u8>
    wal: Vec<u8>
    shm: SqliteShm

    rollback_local_changes()
      reset wal & shm

    fast_forward(changes: Changeset)
      apply changes to main

client:
  client_id: id.new()
  local: Local.new(client_id, mutator)

  // run local mutations
  local.run(AddTodo)
  local.run(UpdateTodo)

  // pull a batch and send to server
  batch: logical.Batch = local.pull(10)
  network.send(batch)

  // receive changes from the server
  cursor = local.cursor()
  changeset: Changeset = network.receive(cursor)

  // rebase on server changes
  local.rebase(changeset)
    // storage.rollback_local_changes()
    // storage.fast_forward(changeset)
    // timeline.rebase(db)

server:
  remote: Remote.new(mutator)

  // receive mutations from client
  batch = network.receive()
  remote.replay(batch)
    // replayer.run(db, batch)

  // if our wal is too long, checkpoint
  remote.maybe_checkpoint()

  // a client wants to pull changes
  cursor = network.receive()
  network.send(remote.diff(cursor))
```