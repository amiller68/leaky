use std::io::Write;
use std::path::PathBuf;

use leaky_common::prelude::*;

use super::change_log::{ChangeLog, ChangeType};

use super::utils;

pub async fn file_needs_pull(leaky: &Leaky, path: &PathBuf, cid: &Cid) -> Result<bool, PullError> {
    if !path.exists() {
        return Ok(true);
    } else if path.is_dir() {
        return Err(PullError::PathIsDirectory(path.clone()));
    }

    let hash = utils::hash_file(path, leaky).await?;
    if hash == *cid {
        Ok(false)
    } else {
        Ok(true)
    }
}

pub async fn pull_file(leaky: &Leaky, path: &PathBuf) -> Result<(), PullError> {
    let data_vec = leaky.cat(&PathBuf::from("/").join(path)).await?;
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

pub async fn pull() -> Result<Cid, PullError> {
    let (mut leaky, _) = utils::load_on_disk().await?;
    let root_cid = leaky.pull_root_cid().await?;
    leaky.pull(&root_cid).await?;

    let pulled_items = leaky
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
                } else if file_needs_pull(&leaky, &ci_path, pi_cid).await?
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
        pull_file(&leaky, item.0).await?;
    }

    for path in to_prune {
        rm_file(&path)?;
    }

    utils::save_on_disk(&mut leaky, &change_log).await?;
    Ok(root_cid)
}

#[derive(Debug, thiserror::Error)]
pub enum PullError {
    #[error("default error: {0}")]
    Default(#[from] anyhow::Error),
    #[error("fs-tree error: {0}")]
    FsTree(#[from] fs_tree::Error),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("could not parse diff: {0}")]
    Serde(#[from] serde_json::Error),
    #[error("leaky error: {0}")]
    Leaky(#[from] LeakyError),
    #[error("path is a directory: {0}")]
    PathIsDirectory(PathBuf),
}
