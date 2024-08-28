use url::Url;

use leaky_common::prelude::*;

use super::utils;

pub async fn init(ipfs_rpc_url: Url, leaky_api_url: Url) -> Result<Cid, InitError> {
    let mut leaky = utils::init_on_disk(ipfs_rpc_url, leaky_api_url, None).await?;
    leaky.push().await?;
    let cid = leaky.cid()?;
    Ok(cid)
}

#[derive(Debug, thiserror::Error)]
pub enum InitError {
    #[error("default error: {0}")]
    Default(#[from] anyhow::Error),
    #[error("leaky error: {0}")]
    Leaky(#[from] LeakyError),
}
