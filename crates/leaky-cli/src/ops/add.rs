use std::fmt::Display;
use std::fs::File;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::SystemTime;

use async_trait::async_trait;

use leaky_common::prelude::*;

use crate::change_log::ChangeType;
use crate::{AppState, Op};

use super::diff::{diff, DiffError};

#[derive(Debug, clap::Args, Clone)]
pub struct Add {
    #[clap(short, long)]
    pub verbose: bool,
}

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
    #[error("invalid schema file: {0}")]
    InvalidSchema(String),
}

fn abs_path(path: &PathBuf) -> Result<PathBuf, DiffError> {
    let path = PathBuf::from("/").join(path);
    Ok(path)
}

#[derive(Debug)]
pub struct AddOutput {
    pub previous_cid: Cid,
    pub cid: Cid,
}

impl Display for AddOutput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.previous_cid == self.cid {
            write!(f, "No changes to add")
        } else {
            write!(f, "{} -> {}", self.previous_cid, self.cid)
        }
    }
}

#[async_trait]
impl Op for Add {
    type Error = AddError;
    type Output = AddOutput;

    async fn execute(&self, state: &AppState) -> Result<Self::Output, Self::Error> {
        let mut client = state.client()?;
        let cid = *state.cid();
        let mut change_log = state.change_log().clone();
        let ipfs_rpc = Arc::new(client.ipfs_rpc()?);
        let mut mount = Mount::pull(cid, &ipfs_rpc).await?;
        let mut updates = diff(&mut change_log).await?;
        let schema_updates = updates.schema().clone();
        let schema_change_log_iter = schema_updates.iter().map(|(path, (hash, change))| {
            let abs_path = abs_path(path).unwrap();
            (path.clone(), abs_path, (hash, change))
        });
        let object_updates = updates.object().clone();
        let object_change_log_iter = object_updates.iter().map(|(path, (hash, change))| {
            let abs_path = abs_path(path).unwrap();
            (path.clone(), abs_path, (hash, change))
        });
        let regular_updates = updates.regular().clone();
        let change_log_iter = regular_updates.iter().map(|(path, (hash, change))| {
            let abs_path = abs_path(path).unwrap();
            (path.clone(), abs_path, (hash, change))
        });

        // First pass - handle schemas
        for (path, abs_path, (hash, diff_type)) in schema_change_log_iter {
            // Read and parse schema file
            let schema_str = std::fs::read_to_string(path.clone())?;
            let schema: Schema = serde_json::from_str(&schema_str)
                .map_err(|e| AddError::InvalidSchema(e.to_string()))?;

            // NOTE: We're gauranteed to have a parrent dir
            // Get the parent directory path for the schema
            let parent_dir = abs_path.parent().ok_or_else(|| {
                AddError::Default(anyhow::anyhow!("Schema file has no parent directory"))
            })?;

            match diff_type {
                ChangeType::Added { modified: true, .. } => {
                    // Add schema to the parent directory with persistence flag true
                    mount.set_schema(parent_dir, schema).await?;
                    if self.verbose {
                        println!(" -> setting schema @ {}", parent_dir.display());
                    }
                    updates.insert(
                        path.clone(),
                        (
                            *hash,
                            ChangeType::Added {
                                modified: false,
                                last_check: Some(SystemTime::now()),
                            },
                        ),
                    );
                }
                ChangeType::Modified {
                    processed: false, ..
                } => {
                    // Add schema to the parent directory with persistence flag true
                    mount.set_schema(parent_dir, schema).await?;
                    if self.verbose {
                        println!(" -> updating schema @ {}", parent_dir.display());
                    }
                    updates.insert(
                        path.clone(),
                        (
                            *hash,
                            ChangeType::Modified {
                                processed: true,
                                last_check: Some(SystemTime::now()),
                            },
                        ),
                    );
                }
                ChangeType::Removed {
                    processed: false, ..
                } => {
                    // Remove schema from the parent directory
                    mount.unset_schema(parent_dir).await?;
                    if self.verbose {
                        println!(" -> removing schema @ {}", parent_dir.display());
                    }
                    updates.insert(
                        path.clone(),
                        (*hash, ChangeType::Removed { processed: true }),
                    );
                }
                _ => {}
            }
        }

        // Second pass - handle regular files
        for (path, abs_path, (hash, diff_type)) in change_log_iter {
            let path_clone = path.clone();
            match diff_type {
                ChangeType::Added { modified: true, .. } => {
                    // read the file and add it to the fucking mount

                    let file = File::open(path)?;
                    if self.verbose {
                        println!(" -> adding file @ {}", abs_path.display());
                    }
                    mount.add(&abs_path, (file, false)).await?;
                    updates.insert(
                        path_clone,
                        (
                            *hash,
                            ChangeType::Added {
                                modified: false,
                                last_check: Some(SystemTime::now()),
                            },
                        ),
                    );
                }
                ChangeType::Modified {
                    processed: false, ..
                } => {
                    // read the file and add it to the fucking mount
                    let file = File::open(path)?;
                    if self.verbose {
                        println!(" -> updating file @ {}", abs_path.display());
                    }
                    mount.add(&abs_path, (file, false)).await?;
                    updates.insert(
                        path_clone,
                        (
                            *hash,
                            ChangeType::Modified {
                                processed: true,
                                last_check: Some(SystemTime::now()),
                            },
                        ),
                    );
                }
                ChangeType::Removed {
                    processed: false, ..
                } => {
                    mount.rm(&abs_path).await?;
                    if self.verbose {
                        println!(" -> removing file @ {}", abs_path.display());
                    }
                    updates.insert(path_clone, (*hash, ChangeType::Removed { processed: true }));
                }
                _ => {}
            }
        }

        // Third pass - handle objects
        for (path, abs_path, (hash, diff_type)) in object_change_log_iter {
            // Get filename and verify format (.name.json)
            let file_name = path
                .file_name()
                .and_then(|f| f.to_str())
                .and_then(|s| s.strip_suffix(".json"))
                .and_then(|s| s.strip_prefix("."))
                .ok_or_else(|| {
                    AddError::Default(anyhow::anyhow!("Object files must be named .name.json"))
                })?;

            // For path/to/dir/.obj/.name.json, construct path/to/dir/name
            let target_path = if let Some(obj_dir) = abs_path.parent() {
                if let Some(parent_dir) = obj_dir.parent() {
                    parent_dir.join(file_name)
                } else {
                    return Err(AddError::Default(anyhow::anyhow!("Invalid object path")));
                }
            } else {
                return Err(AddError::Default(anyhow::anyhow!("Invalid object path")));
            };

            match diff_type {
                ChangeType::Added { modified: true, .. } => {
                    let obj_str = std::fs::read_to_string(path.clone())?;
                    let object: Object = serde_json::from_str(&obj_str)
                        .map_err(|e| AddError::InvalidSchema(e.to_string()))?;
                    let object_clone = object.clone();
                    // write back out in case we upserted created_at and updated_at
                    let obj_str = serde_json::to_string_pretty(&object_clone)?;
                    std::fs::write(path.clone(), obj_str)?;
                    mount.tag(&target_path, object_clone).await?;
                    if self.verbose {
                        println!(" -> adding tag @ {}", target_path.display());
                    }
                    updates.insert(
                        path.clone(),
                        (
                            *hash,
                            ChangeType::Added {
                                modified: false,
                                last_check: Some(SystemTime::now()),
                            },
                        ),
                    );
                }
                ChangeType::Modified {
                    processed: false, ..
                } => {
                    let obj_str = std::fs::read_to_string(path.clone())?;
                    let object: Object = serde_json::from_str(&obj_str)
                        .map_err(|e| AddError::InvalidSchema(e.to_string()))?;
                    let object_clone = object.clone();
                    mount.tag(&target_path, object_clone).await?;
                    if self.verbose {
                        println!(" -> updating tag @ {}", target_path.display());
                    }
                    updates.insert(
                        path.clone(),
                        (
                            *hash,
                            ChangeType::Modified {
                                processed: true,
                                last_check: Some(SystemTime::now()),
                            },
                        ),
                    );
                }
                ChangeType::Removed {
                    processed: false, ..
                } => {
                    mount.rm_tag(&target_path).await?;
                    if self.verbose {
                        println!(" -> removing tag @ {}", target_path.display());
                    }
                    updates.insert(
                        path.clone(),
                        (*hash, ChangeType::Removed { processed: true }),
                    );
                }
                _ => {}
            }
        }

        // TODO: we really shouldn't need to push here
        //  I think the reason we are is so that we can persist
        //  the changes to the mount soooomewhere
        //  Ideally we should be able to write the current state of the mount
        //  locally and only push when we want to
        mount.push().await?;
        let new_cid = *mount.cid();

        state.save(&mount, Some(&updates), None)?;

        if new_cid == cid {
            return Ok(AddOutput {
                previous_cid: cid,
                cid: new_cid,
            });
        }

        Ok(AddOutput {
            previous_cid: cid,
            cid: new_cid,
        })
    }
}
