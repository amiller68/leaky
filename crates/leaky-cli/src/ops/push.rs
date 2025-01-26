use std::sync::Arc;

use async_trait::async_trait;

use leaky_common::prelude::*;

use crate::change_log::ChangeType;
use crate::{AppState, Op};

#[derive(Debug, clap::Args, Clone)]
pub struct Push;

#[derive(Debug, thiserror::Error)]
pub enum PushError {
    #[error("default error: {0}")]
    Default(#[from] anyhow::Error),
    #[error("encountered mismatched cid: {0} != {1}")]
    CidMismatch(Cid, Cid),
    #[error("fs-tree error: {0}")]
    FsTree(#[from] fs_tree::Error),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("could not parse diff: {0}")]
    Serde(#[from] serde_json::Error),
    #[error("could not strip prefix: {0}")]
    PathPrefix(#[from] std::path::StripPrefixError),
    #[error("device error: {0}")]
    Mount(#[from] MountError),
    #[error("api error: {0}")]
    Api(#[from] leaky_common::error::ApiError),
    #[error("app state error: {0}")]
    AppState(#[from] crate::state::AppStateSetupError),
}

#[async_trait]
impl Op for Push {
    type Error = PushError;
    type Output = Cid;

    async fn execute(&self, state: &AppState) -> Result<Self::Output, Self::Error> {
        let mut client = state.client()?;
        let cid = *state.cid();
        let previous_cid = *state.previous_cid();

        if cid == previous_cid {
            println!("No changes to push");
            return Ok(cid);
        }

        let mut change_log = state.change_log().clone();
        let ipfs_rpc = Arc::new(client.ipfs_rpc()?);
        println!("pulling cid: {:?}", cid);
        let mut mount = Mount::pull(cid, &ipfs_rpc).await?;

        println!("pushing cid: {:?}", cid);
        mount.set_previous(previous_cid);
        mount.push().await?;
        let cid = *mount.cid();

        let push_root_req = PushRoot {
            cid: cid.to_string(),
            previous_cid: previous_cid.to_string(),
        };
        println!("Pushing root: {:?}", push_root_req);
        client.call(push_root_req).await?;

        let mut updates = change_log.clone();
        // Update the changelog to drop removed, and set everything else to base
        let change_log_iter = change_log.iter_mut();
        for (path, (hash, diff_type)) in change_log_iter {
            match diff_type {
                ChangeType::Removed => {
                    updates.remove(path);
                }
                _ => {
                    updates.insert(path.clone(), (*hash, ChangeType::Base));
                }
            }
        }

        state.save(&mount, Some(&updates), Some(cid))?;

        Ok(cid)
    }
}
