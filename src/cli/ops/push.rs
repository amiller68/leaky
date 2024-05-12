use std::fs::File;

use leaky::prelude::*;

use super::change_log::ChangeType;
use super::utils;

pub async fn push() -> Result<Cid, PushError> {
    let (mut leaky, change_log) = utils::load_on_disk().await?;

    let mut updates = change_log.clone();

    let root_cid = leaky.cid()?;

    let mut changed = false;
    let change_log_iter = change_log.iter();
    // Iterate over the ChangeLog -- play updates against the base ... probably better to do this
    for (path, (hash, diff_type)) in change_log_iter {
        match diff_type {
            ChangeType::Added { .. } => {
                changed = true;
                let file = File::open(&path)?;
                leaky.add_data(file).await?;
                updates.insert(path.clone(), (*hash, ChangeType::Base));
            }

            ChangeType::Modified => {
                changed = true;
                let file = File::open(&path)?;
                leaky.add_data(file).await?;
                updates.insert(path.clone(), (*hash, ChangeType::Base));
            }

            ChangeType::Removed => {
                changed = true;
                updates.insert(path.clone(), (*hash, ChangeType::Base));
            }

            _ => {}
        }
    }

    if !changed {
        println!("No added changes to push");
        return Ok(root_cid);
    }

    leaky.push().await?;

    let root_cid = leaky.cid()?;

    utils::save_on_disk(&mut leaky, &updates).await?;

    Ok(root_cid)
}

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
    Leaky(#[from] LeakyError),
}
