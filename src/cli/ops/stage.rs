use std::fs::File;

use leaky::prelude::*;

use super::diff::{diff, DiffError};

use super::change_log::ChangeType;
use super::utils;

pub async fn stage() -> Result<(), StageError> {
    let mut leaky = utils::load_leaky().await?;
    let updates = diff().await?;
    let mut change_log = utils::load_change_log()?;
    let root_cid = leaky.cid()?;

    let change_log_iter = updates.iter();
    // Iterate over the ChangeLog -- play updates against the base ... probably better to do this
    for (path, (_hash, diff_type)) in change_log_iter {
        // Skip unchanged files -- mark changed files as base
        if diff_type == &ChangeType::Base {
            continue;
        }
        // updates.insert(path.clone(), (cid.clone(), ChangeType::Staged));

        if diff_type == &ChangeType::Added || diff_type == &ChangeType::Modified {
            let file = File::open(&path)?;
            leaky.add(path, file, None).await?;
        }

        // If the file is a file, we just remove it from the Manifest
        // It won't be visible, but should be within the Fs History
        if diff_type == &ChangeType::Removed {
            println!("we don't support removing files yet: {}", path.display());
        }
    }

    let new_root_cid = leaky.cid()?;

    if new_root_cid == root_cid {
        println!("No changes to stage");
        return Ok(());
    }

    utils::save_change_log(&change_log)?;

    Ok(())
}

#[derive(Debug, thiserror::Error)]
pub enum StageError {
    #[error("default error")]
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
