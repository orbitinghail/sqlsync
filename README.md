timeline:
 - 

client side:

 - instance of a db at snapshot id X
 - registered set of named mutations
 - list of pending mutations that have been applied locally
 - connection to server
   - listening for updates
     - replace local snapshot
     - invalidate applied local mutations
     - replay unapplied local mutations
   - send mutations

server side:
 - instance of the db at snapshot id X
 - registered set of named mutations
 - connection to clients
   - receive mutations
     - apply to db (potentially batched)
     - send diffs to clients