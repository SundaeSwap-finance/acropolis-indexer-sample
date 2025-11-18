# Acropolis Indexer Sketch

Sample code for what an Acropolis-backed indexer could look like.

The indexer would track several independent indexes. The interface to an index looks like this:

```rs
// Managed indexes are written in an "event handler" style.
// They react to a stream of events, starting at a configured point on the chain.
// Each index can be somewhere different on-chain, so they should be granular.
#[async_trait]
pub trait ManagedIndex: Send + Sync + 'static {
    fn name(&self) -> String;

    // Called when a new TX has arrived on-chain.
    async fn handle_onchain_tx(&mut self, info: &BlockInfo, tx: &MultiEraTx) -> Result<()> {
        // This method can update a database, or a mutex-locked in-memory map, or publish messages to the rest of the system,
        // or whatever. It's async and it's allowed to be long-running.
        let _ = (info, tx);
        // These indexes are fallible. If an index fails, we'll stop updating it, but keep running other indexes.
        // Could make sense to build a retry in too, in case the issue is just a fallible DB
        Ok(())
    }

    // Called when a block has rolled back.
    async fn handle_rollback(&mut self, info: &BlockInfo) -> Result<()> {
        let _ = info;
        Ok(())
    }
}
```

We'd build one of these "indexes" for each set of data we cared to track in the system. The acropolis module can send updates them as they arrive.

The chain indexer itself would be an Acropolis module, but one with an imperative interface which we could use to set it up. The rest of the scooper doesn't need to use the Acropolis message bus or anything, this module would run in-process so we could just use mutexes and channels and all that.
```rs
let mut indexer = ChainIndexer::new(InMemoryCursorStore::new(vec![]));
let starting_point = match args.command {
    Commands::SyncFromOrigin => Point::Origin,
    Commands::SyncFromPoint{ slot, block_hash } => Point::Specific(slot, block_hash.0)
};
let force_rebuild = false;
indexer.add_index(PoolIndex::new(), starting_point.clone(), force_rebuild);
indexer.add_index(OrderIndex::new(), starting_point.clone(), force_rebuild);

let mut process = Process::create();
process.register(indexer);
process.run().await.unwrap();
```

Each index has a "cursor" tracking how far it is on the chain. If we have new sets of data which we want to index, we can create a new index for them, and let that index build itself while the rest of the application chugs along. Different indexes can start at different points on-chain, so if we introduce e.g. a new type of pool we don't have to search for it starting from the Alonzo era.

