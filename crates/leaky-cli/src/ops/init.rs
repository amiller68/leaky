use crate::{AppState, Op};
use async_trait::async_trait;
use leaky_common::prelude::*;
use std::sync::Arc;
use url::Url;

#[derive(Debug, clap::Args, Clone)]
pub struct Init {
    // NOTE: not used in exexute, but when initializing the app state
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
    Api(#[from] leaky_common::error::ApiError),
    #[error("mount error: {0}")]
    Mount(#[from] leaky_common::error::MountError),
    #[error("remote already initialized")]
    RemoteAlreadyInitialized,
}

#[async_trait]
impl Op for Init {
    type Error = InitError;
    type Output = Cid;

    async fn execute(&self, state: &AppState) -> Result<Self::Output, Self::Error> {
        let mut client = state.client()?;
        let ipfs_rpc = Arc::new(client.ipfs_rpc()?);

        let mut mount = Mount::init(&ipfs_rpc.clone()).await?;
        mount.push().await?;

        let previous_cid = Cid::default().to_string();
        let cid = mount.cid().to_string();

        let push_root = PushRoot { cid, previous_cid };
        match client.call(push_root).await {
            Ok(_) => {}
            Err(e) => match e {
                leaky_common::error::ApiError::HttpStatus(_status, text) => {
                    if text == "invalid link" {
                        println!("remote already initialized");
                    }
                }
                _ => return Err(InitError::Api(e)),
            },
        }

        state.save(&mount, None, Some(*mount.cid()))?;

        Ok(mount.cid().clone())
    }
}
