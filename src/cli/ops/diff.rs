use std::fs::File;
use std::path::PathBuf;

use leaky::prelude::*;

use super::change_log::{ChangeLog, ChangeType};
use super::utils;

pub async fn diff() -> Result<ChangeLog, DiffError> {
    println!("Diffing...");
    let change_log = utils::load_change_log()?;
    let mut base = change_log.clone();
    let mut update = base.clone();
    let next = utils::fs_tree()?;
    let default_hash = blake3::Hasher::new().finalize();
    println!("Diffing...");

    // Insert the root directory hash into the change_log for comparison
    // This should always just get matched out and removed
    base.insert(PathBuf::from(""), (default_hash, ChangeType::Base));

    // Iterate over the path-sorted change_log and the fs-tree in order to diff
    let mut base_iter = base.iter();
    let mut next_iter = next.iter();

    println!("Diffing...");
    let mut next_next = next_iter.next();
    let mut base_next = base_iter.next();

    loop {
        match (next_next.clone(), base_next) {
            // If these are both something we got some work to do
            (Some((_next_tree, next_path)), Some((base_path, (base_hash, base_type)))) => {
                // For each object, assuming we stay aligned on a sorted list of paths:
                // If the base comes before then this file was removed
                // strip off the base object and log the removal
                if base_path < &next_path {
                    if !base_path.is_dir() {
                        match base_type {
                            ChangeType::Added => {
                                update.remove(base_path);
                            }
                            _ => {
                                update
                                    .insert(base_path.clone(), (default_hash, ChangeType::Removed));
                            }
                        }
                    }
                    base_next = base_iter.next();
                    continue;
                }

                // If next comes before base then the file was added
                // strip off the next object and log the addition
                if &next_path < base_path {
                    if !next_path.is_dir() {
                        println!("Adding {:?}", next_path);
                        let hash = utils::hash_file(&next_path)?;
                        println!("Hashed {:?}", next_path);
                        update.insert(next_path.clone(), (hash, ChangeType::Added));
                    }
                    next_next = next_iter.next();
                    continue;
                }

                // If they are equal then we are good. Move on to the next objects
                if &next_path == base_path {
                    // These are either both files or both directories
                    // If they are both files then we need to compare hashes
                    if !next_path.is_dir() {
                        println!("Comparing {:?}", next_path);
                        // If the hashes are different then the file was modified
                        // strip off the next object and log the modification
                        let next_hash = utils::hash_file(&next_path)?;
                        println!("Hashed {:?}", next_path);
                        if base_hash != &next_hash {
                            match base_type {
                                ChangeType::Added => {
                                    update
                                        .insert(base_path.clone(), (next_hash, ChangeType::Added));
                                }
                                _ => {
                                    update.insert(
                                        base_path.clone(),
                                        (next_hash, ChangeType::Modified),
                                    );
                                }
                            }
                        }
                    }

                    next_next = next_iter.next();
                    base_next = base_iter.next();
                    continue;
                }
            }

            // Theres more old file than new, this file was removed
            (Some((_next_tree, next_path)), None) => {
                if !next_path.is_dir() {
                    println!("Adding {:?}", next_path);
                    let hash = utils::hash_file(&next_path)?;
                    println!("Hashed {:?}", next_path);
                    update.insert(next_path.clone(), (hash, ChangeType::Added));
                }
                next_next = next_iter.next();
                continue;
            }

            // There's more new files than old, this file was added
            (None, Some((base_path, (_base_hash, base_type)))) => {
                if !base_path.is_dir() {
                    match base_type {
                        ChangeType::Added => {
                            update.remove(base_path);
                        }
                        _ => {
                            update.insert(base_path.clone(), (default_hash, ChangeType::Removed));
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

    println!("Diffing...");
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
