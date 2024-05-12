use std::path::PathBuf;

use leaky::prelude::*;

use super::change_log::{ChangeLog, ChangeType};
use super::utils;

pub async fn diff(leaky: &Leaky, base: &mut ChangeLog) -> Result<ChangeLog, DiffError> {
    let base: &mut ChangeLog = base;
    let mut update = base.clone();
    let next = utils::fs_tree()?;
    let default_hash = Cid::default();

    // Insert the root directory hash into the change_log for comparison
    // This should always just get matched out and removed
    base.insert(PathBuf::from(""), (default_hash, ChangeType::Base));

    // Iterate over the path-sorted change_log and the fs-tree in order to diff
    // Map all paths to absolute paths within the root directory
    let mut base_iter = base
        .iter()
        .map(|(path, (hash, change))| (path.clone(), (hash, change)));
    let mut next_iter = next.iter();

    let mut next_next = next_iter.next();
    let mut base_next = base_iter.next();
    loop {
        match (next_next.clone(), base_next.clone()) {
            // If these are both something we got some work to do
            (Some((next_tree, next_path)), Some((base_path, (base_hash, base_type)))) => {
                println!("{:?} {:?}", next_path, base_path);
                // For each object, assuming we stay aligned on a sorted list of paths:
                // If the base comes before then this file was removed
                // strip off the base object and log the removal
                if base_path < next_path {
                    println!("base_path < next_path");
                    if !base_path.is_dir() {
                        println!("not dir");
                        match base_type {
                            ChangeType::Added { .. } => {
                                update.remove(&base_path);
                            }
                            _ => {
                                update.insert(base_path, (default_hash, ChangeType::Removed));
                            }
                        }
                    }
                    base_next = base_iter.next();
                    continue;
                }

                // If next comes before base then the file was added
                // strip off the next object and log the addition
                if next_path < base_path {
                    println!("next_path < base_path");
                    if !next_path.is_dir() {
                        println!("not dir");
                        let hash = utils::hash_file(&next_path, leaky).await?;
                        update.insert(
                            next_path.clone(),
                            (hash, ChangeType::Added { modified: true }),
                        );
                    }
                    next_next = next_iter.next();
                    continue;
                }

                // If they are equal then we are good. Move on to the next objects
                if next_path == base_path {
                    println!("equal");
                    // These are either both files or both directories
                    // If they are both files then we need to compare hashes
                    if !next_tree.is_dir() {
                        println!("not dir");
                        // If the hashes are different then the file was modified
                        // strip off the next object and log the modification
                        let next_hash = utils::hash_file(&next_path, leaky).await?;
                        if base_hash != &next_hash {
                            match base_type {
                                // If it has been added before, then it was modified
                                ChangeType::Added { .. } => {
                                    update.insert(
                                        base_path.clone(),
                                        (next_hash, ChangeType::Added { modified: true }),
                                    );
                                }
                                // Otherwise it was just modified
                                _ => {
                                    update.insert(
                                        base_path.clone(),
                                        (next_hash, ChangeType::Modified),
                                    );
                                }
                            }
                        } else if let ChangeType::Added { .. } = base_type {
                            update.insert(
                                base_path.clone(),
                                (next_hash, ChangeType::Added { modified: false }),
                            );
                        }
                    }

                    next_next = next_iter.next();
                    base_next = base_iter.next();
                    continue;
                }
            }

            // Theres more new files than old, so this file was added
            (Some((next_tree, next_path)), None) => {
                println!("{:?} | EMPTY", next_path);
                if !next_tree.is_dir() {
                    println!("not dir");
                    let hash = utils::hash_file(&next_path, leaky).await?;
                    update.insert(
                        next_path.clone(),
                        (hash, ChangeType::Added { modified: true }),
                    );
                }
                next_next = next_iter.next();
                continue;
            }

            // There's more old files than new, so this file was removed
            (None, Some((base_path, (_base_hash, base_type)))) => {
                println!("EMPTY | {:?}", base_path);
                if !base_path.is_dir() {
                    println!("not dir");
                    match base_type {
                        ChangeType::Added { .. } => {
                            update.remove(&base_path);
                        }
                        _ => {
                            update.insert(base_path, (default_hash, ChangeType::Removed));
                        }
                    }
                }
                base_next = base_iter.next();
                continue;
            }
            (None, None) => {
                // We are done
                break;
            }
        }
    }

    Ok(update)
}

#[derive(Debug, thiserror::Error)]
pub enum DiffError {
    #[error("default error: {0}")]
    Default(#[from] anyhow::Error),
    #[error("could not read change_log: {0}")]
    ReadChanges(#[from] serde_json::Error),
    #[error("fs-tree error: {0}")]
    FsTree(#[from] fs_tree::Error),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("leaky error: {0}")]
    Leaky(#[from] LeakyError),
    #[error("file does not exist")]
    PathDoesNotExist(PathBuf),
    #[error("path is a directory")]
    PathIsDirectory(PathBuf),
}
