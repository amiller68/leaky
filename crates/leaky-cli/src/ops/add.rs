use std::fmt::Display;
use std::fs::File;
use std::path::{Path, PathBuf};
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
    #[error("invalid schema file: {0}")]
    InvalidSchema(String),
}

fn abs_path(path: &PathBuf) -> Result<PathBuf, DiffError> {
    let path = PathBuf::from("/").join(path);
    Ok(path)
}

async fn handle_schema_file(
    mount: &mut Mount,
    path: &PathBuf,
    abs_path: &Path,
) -> Result<(), AddError> {
    // Read and parse schema file
    let schema_str = std::fs::read_to_string(path)?;
    let schema: Schema =
        serde_json::from_str(&schema_str).map_err(|e| AddError::InvalidSchema(e.to_string()))?;

    // Get the parent directory path for the schema
    let parent_dir = abs_path
        .parent()
        .ok_or_else(|| AddError::Default(anyhow::anyhow!("Schema file has no parent directory")))?;

    // Add schema to the parent directory with persistence flag true
    mount.set_schema(parent_dir, schema).await?;

    Ok(())
}

async fn handle_object_file(
    mount: &mut Mount,
    path: &PathBuf,
    abs_path: &Path,
) -> Result<(), AddError> {
    // Read and parse object file
    let obj_str = std::fs::read_to_string(path)?;
    let object: Object =
        serde_json::from_str(&obj_str).map_err(|e| AddError::InvalidSchema(e.to_string()))?;
    // write back out in case we upserted created_at and updated_at
    let obj_str = serde_json::to_string_pretty(&object)?;
    std::fs::write(path, obj_str)?;

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

    // Add object to the target file
    mount.tag(&target_path, object).await?;

    Ok(())
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

        let updates = diff(&mut change_log).await?;
        let change_log_iter = updates.iter().map(|(path, (hash, change))| {
            let abs_path = abs_path(path).unwrap();
            (path.clone(), abs_path, (hash, change))
        });

        // First pass - handle schemas
        for (path, abs_path, (_hash, diff_type)) in change_log_iter.clone() {
            if path.file_name().map_or(false, |f| f == ".schema") {
                match diff_type {
                    ChangeType::Added { .. } | ChangeType::Modified => {
                        handle_schema_file(&mut mount, &path, &abs_path).await?;
                    }
                    ChangeType::Removed => {
                        // TODO: Schema removal not yet supported
                    }
                    _ => continue,
                }
            }
        }

        // Second pass - handle regular files
        for (path, abs_path, (_hash, diff_type)) in change_log_iter.clone() {
            let file_name = path.file_name().map(|f| f.to_string_lossy().to_string());

            // Skip schema and object files -- files named .schema and files who's parent is .obj
            // only check obj if the parent has enough path segments to be a .obj directory
            let maybe_parent = path.parent();
            if file_name.as_deref() == Some(".schema")
                || if let Some(parent) = maybe_parent {
                    parent.file_name().map_or(false, |f| f == ".obj")
                } else {
                    false
                }
            {
                continue;
            }

            match diff_type {
                ChangeType::Added { .. } => {
                    let file = File::open(&path)?;
                    mount.add(&abs_path, (file, false)).await?;
                }
                ChangeType::Modified => {
                    let file = File::open(&path)?;
                    mount.add(&abs_path, (file, false)).await?;
                }
                ChangeType::Removed => {
                    mount.rm(&abs_path).await?;
                }
                _ => continue,
            }
        }

        // Third pass - handle objects
        for (path, abs_path, (_hash, diff_type)) in change_log_iter {
            // Check if file is in a .obj directory
            if let Some(parent) = path.parent() {
                if parent.file_name().map_or(false, |f| f == ".obj") {
                    match diff_type {
                        ChangeType::Added { .. } | ChangeType::Modified => {
                            handle_object_file(&mut mount, &path, &abs_path).await?;
                        }
                        _ => continue,
                    }
                }
            }
        }

        mount.push().await?;
        let new_cid = *mount.cid();

        if new_cid == cid {
            return Ok(AddOutput {
                previous_cid: cid,
                cid: new_cid,
            });
        }

        state.save(&mount, Some(&updates), None)?;

        Ok(AddOutput {
            previous_cid: cid,
            cid: new_cid,
        })
    }
}
