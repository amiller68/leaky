use std::fs::File;
use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;

use leaky_common::prelude::*;

use crate::change_log::ChangeType;
use crate::{AppState, Op};

use super::diff::{diff, DiffError};

#[derive(Debug, clap::Args, Clone)]
pub struct Add;

#[derive(Debug, thiserror::Error)]
pub enum AddError {
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
    #[error("diff error: {0}")]
    Diff(#[from] DiffError),
    #[error("mount error: {0}")]
    Mount(#[from] MountError),
    #[error("api error: {0}")]
    Api(#[from] leaky_common::error::ApiError),
    #[error("app state error: {0}")]
    AppState(#[from] crate::state::AppStateSetupError),
}

fn abs_path(path: &PathBuf) -> Result<PathBuf, DiffError> {
    let path = PathBuf::from("/").join(path);
    Ok(path)
}

#[async_trait]
impl Op for Add {
    type Error = AddError;
    type Output = Cid;

    async fn execute(&self, state: &AppState) -> Result<Self::Output, Self::Error> {
        let mut client = state.client()?;
        let cid = state.cid().clone();
        let mut change_log = state.change_log().clone();
        let ipfs_rpc = Arc::new(client.ipfs_rpc()?);
        let mut mount = Mount::pull(cid, &ipfs_rpc).await?;
        let updates = diff(&mount, &mut change_log).await?;

        let change_log_iter = updates.iter().map(|(path, (hash, change))| {
            let abs_path = abs_path(path).unwrap();
            (path.clone(), abs_path, (hash, change))
        });
        // Iterate over the ChangeLog -- play updates against the base ... probably better to do this
        for (path, abs_path, (_hash, diff_type)) in change_log_iter {
            match diff_type {
                ChangeType::Added { modified: true } => {
                    let file = File::open(&path)?;
                    mount.add(&abs_path, file, None, false).await?;
                }

                ChangeType::Modified => {
                    let file = File::open(&path)?;
                    mount.add(&abs_path, file, None, false).await?;
                }

                ChangeType::Removed => {
                    mount.rm(&abs_path).await?;
                }

                _ => {
                    // Skip unchanged files
                    continue;
                }
            }
        }

        let new_cid = mount.cid().clone();

        if new_cid == cid {
            println!("No changes to add");
            return Ok(cid);
        }

        mount.push().await?;

        state.save(&mut mount, Some(&updates), None)?;

        Ok(new_cid)
    }
}
