use std::fs::File;
use std::path::PathBuf;

use leaky_common::prelude::*;

use super::diff::{diff, DiffError};

use super::change_log::ChangeType;
use super::utils;

fn abs_path(path: &PathBuf) -> Result<PathBuf, DiffError> {
    let path = PathBuf::from("/").join(path);
    Ok(path)
}

pub async fn add() -> Result<Cid, AddError> {
    let (mut leaky, mut change_log) = utils::load_on_disk().await?;

    // Diff against the cwd
    let updates = diff(&leaky, &mut change_log).await?;

    let root_cid = leaky.cid()?;

    let change_log_iter = updates.iter().map(|(path, (hash, change))| {
        let abs_path = abs_path(path).unwrap();
        (path.clone(), abs_path, (hash, change))
    });
    // Iterate over the ChangeLog -- play updates against the base ... probably better to do this
    for (path, abs_path, (_hash, diff_type)) in change_log_iter {
        match diff_type {
            ChangeType::Added { modified: true } => {
                let file = File::open(&path)?;
                leaky.add(&abs_path, file, None, true).await?;
            }

            ChangeType::Modified => {
                let file = File::open(&path)?;
                leaky.add(&abs_path, file, None, true).await?;
            }

            ChangeType::Removed => {
                leaky.rm(&abs_path).await?;
            }

            _ => {
                // Skip unchanged files
                continue;
            }
        }
    }

    let new_root_cid = leaky.cid()?;

    if new_root_cid == root_cid {
        println!("No changes to add");
        return Ok(root_cid);
    }

    utils::save_on_disk(&mut leaky, &updates).await?;

    Ok(new_root_cid)
}

#[derive(Debug, thiserror::Error)]
pub enum AddError {
    #[error("default error: {0}")]
    Default(#[from] anyhow::Error),
    #[error("cid error: {0}")]
    Cid(#[from] libipld::cid::Error),
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
    #[error("device error: {0}")]
    Leaky(#[from] LeakyError),
}
