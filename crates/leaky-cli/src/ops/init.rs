use crate::{api::api_requests::PushRoot, AppState, Op};
use async_trait::async_trait;
use leaky_common::prelude::*;
use std::sync::Arc;
use url::Url;

#[derive(Debug, clap::Args, Clone)]
pub struct Init {
    #[clap(short, long)]
    pub remote: Url,
}

#[derive(Debug, thiserror::Error)]
pub enum InitError {
    #[error("default error: {0}")]
    Default(#[from] anyhow::Error),

    #[error("app state error: {0}")]
    AppState(#[from] crate::state::AppStateSetupError),

    #[error("ipfs error: {0}")]
    Ipfs(#[from] leaky_common::error::IpfsRpcError),

    #[error("api error: {0}")]
    Api(#[from] crate::api::ApiError),

    #[error("mount error: {0}")]
    Mount(#[from] leaky_common::error::MountError),
}

#[async_trait]
impl Op for Init {
    type Error = InitError;
    type Output = Cid;

    async fn execute(&self, state: &AppState) -> Result<Self::Output, Self::Error> {
        let mut client = state.client()?;
        let ipfs_rpc = Arc::new(client.ipfs_rpc()?);

        let mount = Mount::init(&ipfs_rpc.clone()).await?;
        mount.push().await?;

        let previous_cid = Cid::default().to_string();
        let cid = mount.cid().to_string();

        let push_root = PushRoot { cid, previous_cid };
        client.call(push_root).await?;

        state.save(&mount, None)?;

        Ok(mount.cid().clone())
    }
}
