use std::io::Write;
use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;

use leaky_common::prelude::*;

use crate::change_log::ChangeLog;
use crate::change_log::ChangeType;
use crate::{AppState, Op};

use super::utils;

#[derive(Debug, clap::Args, Clone)]
pub struct Pull;

#[derive(Debug, thiserror::Error)]
pub enum PullError {
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
    #[error("mount error: {0}")]
    Mount(#[from] MountError),
    #[error("api error: {0}")]
    Api(#[from] leaky_common::error::ApiError),
    #[error("app state error: {0}")]
    AppState(#[from] crate::state::AppStateSetupError),
    #[error("path is a directory: {0}")]
    PathIsDirectory(PathBuf),
}

#[async_trait]
impl Op for Pull {
    type Error = PullError;
    type Output = Cid;

    async fn execute(&self, state: &AppState) -> Result<Self::Output, Self::Error> {
        let mut client = state.client()?;
        let pull_root_req = PullRoot {};
        let root_cid = client.call(pull_root_req).await?;
        let cid = root_cid.cid();
        let ipfs_rpc = Arc::new(client.ipfs_rpc()?);
        let mount = Mount::pull(cid, &ipfs_rpc).await?;

        let pulled_items = mount
            .items()
            .await?
            .iter()
            .map(|(path, cid)| (path.strip_prefix("/").unwrap().to_path_buf(), *cid))
            .collect::<Vec<_>>();

        // Insert everything in the change log
        let mut change_log = ChangeLog::new();
        for (path, cid) in pulled_items.iter() {
            change_log.insert(path.clone(), (*cid, ChangeType::Base));
        }

        let current_fs_tree = utils::fs_tree()?;

        let mut pi_iter = pulled_items.iter();
        let mut ci_iter = current_fs_tree.iter();

        // Pop off "" from the fs-tree
        ci_iter.next();

        let mut to_pull = Vec::new();
        let mut to_prune = Vec::new();

        let mut pi_next = pi_iter.next();
        let mut ci_next = ci_iter.next();

        loop {
            match (pi_next, ci_next.clone()) {
                (Some((pi_path, pi_cid)), Some((ci_tree, ci_path))) => {
                    // First check if ci is a dir, since we skip those
                    if ci_tree.is_dir() {
                        ci_next = ci_iter.next();
                        continue;
                    }
                    if pi_path < &ci_path {
                        to_pull.push((pi_path, pi_cid));
                    } else if pi_path > &ci_path {
                        to_prune.push(ci_path);
                    } else if file_needs_pull(&mount, &ci_path, pi_cid).await?
                        && *pi_cid != Cid::default()
                    {
                        to_pull.push((pi_path, pi_cid));
                    }
                    pi_next = pi_iter.next();
                    ci_next = ci_iter.next();
                }
                (Some(pi), None) => {
                    to_pull.push((&pi.0, &pi.1));
                    pi_next = pi_iter.next();
                }
                (None, Some(ci)) => {
                    to_prune.push(ci.1);
                    ci_next = ci_iter.next();
                }
                (None, None) => {
                    break;
                }
            }
        }

        for item in to_pull {
            pull_file(&mount, item.0).await?;
        }

        for path in to_prune {
            rm_file(&path)?;
        }
        let cid = *mount.cid();
        state.save(&mount, Some(&change_log), Some(cid))?;
        Ok(cid)
    }
}

pub async fn file_needs_pull(mount: &Mount, path: &PathBuf, cid: &Cid) -> Result<bool, PullError> {
    if !path.exists() {
        return Ok(true);
    } else if path.is_dir() {
        return Err(PullError::PathIsDirectory(path.clone()));
    }

    let hash = utils::hash_file(path, mount).await?;
    if hash == *cid {
        Ok(false)
    } else {
        Ok(true)
    }
}

pub async fn pull_file(mount: &Mount, path: &PathBuf) -> Result<(), PullError> {
    let data_vec = mount.cat(&PathBuf::from("/").join(path)).await?;
    let mut object_path = path.clone();
    object_path.pop();
    std::fs::create_dir_all(object_path)?;
    let mut file = std::fs::File::create(path)?;
    file.write_all(data_vec.as_slice())?;
    Ok(())
}

fn rm_file(path: &PathBuf) -> Result<(), PullError> {
    std::fs::remove_file(path)?;
    Ok(())
}
