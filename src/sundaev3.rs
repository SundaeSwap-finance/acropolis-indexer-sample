use pallas_primitives::BigInt;
use plutus_parser::AsPlutus;

pub type Ident = Vec<u8>;

pub type AssetClass = (Vec<u8>, Vec<u8>);

#[derive(AsPlutus, Clone)]
pub struct PoolDatum {
    pub ident: Ident,
    pub assets: (AssetClass, AssetClass),
    pub circulating_lp: BigInt,
}
