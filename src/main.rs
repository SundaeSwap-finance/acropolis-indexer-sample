mod acropolis;
mod sundaev3;

use std::collections::{BTreeMap, HashSet};

use anyhow::{Result, anyhow};
use async_trait::async_trait;
use clap::Parser as _;
use pallas_addresses::Address;
use pallas_crypto::hash::Hasher;
use pallas_network::miniprotocols::Point;
use pallas_primitives::conway::{MintedDatumOption, Value};
use pallas_traverse::{MultiEraOutput, MultiEraTx, OutputRef};
use plutus_parser::AsPlutus;

use crate::{
    acropolis::{
        core::{BlockHash, BlockInfo, Process},
        indexer::{ChainIndexer, InMemoryCursorStore, ManagedIndex},
    },
    sundaev3::{Ident, PoolDatum},
};

struct PoolInfo {
    created_at: u64,
    #[allow(unused)]
    datum: PoolDatum,
}
struct PoolIndex {
    // Pretend this is something persistent like a database.
    pools: BTreeMap<Ident, PoolInfo>,
}

impl PoolIndex {
    fn new() -> Self {
        Self {
            pools: BTreeMap::new(),
        }
    }
}

fn parse_datum<T: AsPlutus>(output: &MultiEraOutput, tx: &MultiEraTx) -> Option<T> {
    match output.datum()? {
        MintedDatumOption::Data(d) => T::from_plutus(d.0.unwrap()).ok(),
        MintedDatumOption::Hash(h) => tx.plutus_data().iter().find_map(|d| {
            let hash = Hasher::<256>::hash(d.raw_cbor());
            if hash != h {
                return None;
            }
            T::from_plutus(d.clone().unwrap()).ok()
        }),
    }
}

// Managed indexes are written in an "event handler" style.
// They react to a stream of events, starting at a configured point on the chain.
// Each index can be somewhere different on-chain, so they should be granular.
#[async_trait]
impl ManagedIndex for PoolIndex {
    fn name(&self) -> String {
        "pools".into()
    }

    async fn handle_onchain_tx(&mut self, info: &BlockInfo, tx: &MultiEraTx) -> anyhow::Result<()> {
        for output in tx.outputs() {
            let Some(pd) = parse_datum::<PoolDatum>(&output, tx) else {
                continue;
            };
            // In reality, this would probably be updating a DB
            self.pools.insert(
                pd.ident.clone(),
                PoolInfo {
                    created_at: info.slot,
                    datum: pd,
                },
            );
        }
        // This method is fallible; if it fails, the indexer will stop updating this index.
        Ok(())
    }

    async fn handle_rollback(&mut self, info: &acropolis::core::BlockInfo) -> anyhow::Result<()> {
        self.pools.retain(|_, v| v.created_at < info.slot);
        Ok(())
    }
}

struct WalletIndex {
    address: Address,
    utxos: Vec<(OutputRef, Value)>,
}
impl WalletIndex {
    fn new(address: Address) -> Self {
        Self {
            address,
            utxos: vec![],
        }
    }
}

#[async_trait]
impl ManagedIndex for WalletIndex {
    fn name(&self) -> String {
        "wallet".into()
    }

    async fn handle_onchain_tx(
        &mut self,
        _info: &acropolis::core::BlockInfo,
        tx: &pallas_traverse::MultiEraTx,
    ) -> anyhow::Result<()> {
        let spent = tx
            .inputs()
            .iter()
            .map(|i| i.output_ref())
            .collect::<HashSet<_>>();
        self.utxos.retain(|u| !spent.contains(&u.0));
        for (out_idx, output) in tx.outputs().iter().enumerate() {
            if output.address().is_ok_and(|a| a == self.address) {
                let ref_ = OutputRef::new(tx.hash(), out_idx as u64);
                self.utxos.push((ref_, output.value().into_conway()));
            }
        }
        Ok(())
    }
}

#[derive(clap::Parser, Debug)]
struct Args {
    #[arg(short, long)]
    addr: String,

    #[arg(short, long)]
    magic: u64,

    #[arg(long)]
    wallet_address: Address,

    #[command(subcommand)]
    command: Commands,
}

fn parse_block_hash(bh: &str) -> Result<BlockHash> {
    let bytes = hex::decode(bh)?;

    bytes.try_into().map_err(|b: Vec<u8>| {
        anyhow!(
            "Expected length {} for block hash, but got {}",
            BlockHash::BYTES,
            b.len()
        )
    })
}

#[derive(clap::Subcommand, Debug)]
enum Commands {
    SyncFromOrigin,
    SyncFromPoint {
        #[arg(short, long)]
        slot: u64,

        #[arg(short, long, value_parser=parse_block_hash)]
        block_hash: BlockHash,
    },
}

#[tokio::main]
async fn main() {
    let args = Args::parse();

    let handle = tokio::spawn(async move {
        let mut indexer = ChainIndexer::new(InMemoryCursorStore::new(vec![]));
        let point = match args.command {
            Commands::SyncFromOrigin => Point::Origin,
            Commands::SyncFromPoint { slot, block_hash } => {
                Point::Specific(slot, block_hash.to_vec())
            }
        };
        indexer.add_index(PoolIndex::new(), point.clone(), false);
        indexer.add_index(WalletIndex::new(args.wallet_address), point.clone(), false);

        let mut process = Process::create();
        process.register(indexer);
        process.run().await.unwrap();
    });
    let _ = handle.await;
}
