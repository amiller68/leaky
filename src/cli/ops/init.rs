use url::Url;

use leaky::prelude::*;

use super::utils;

pub async fn init(ipfs_rpc: Url) -> Result<Cid, InitError> {
    let mut leaky = Leaky::new(ipfs_rpc.clone())?;
    leaky.init().await?;
    leaky.push().await?;
    let cid = leaky.cid()?;
    utils::init_leaky_config(ipfs_rpc, cid)?;
    Ok(cid)
}

#[derive(Debug, thiserror::Error)]
pub enum InitError {
    #[error("default error")]
    Default(#[from] anyhow::Error),
    #[error("leaky error")]
    Leaky(#[from] LeakyError),
}
