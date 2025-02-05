use std::io::Write;
use std::path::PathBuf;
use std::sync::Arc;
use std::fs;
use serde_json;

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
        let local_ipfs_rpc = IpfsRpc::default();
        let mount = Mount::pull(cid, &ipfs_rpc).await?;

        let (links, schemas) = mount.ls_with_schemas(&PathBuf::from("/"), true).await?;
        println!("links: {:?}", links);
        
        let pulled_items = links
            .into_iter()
            .filter_map(|(path, link)| {
                // Skip the root path
                if path.as_os_str().is_empty() {
                    return None;
                }
                // Convert absolute paths to relative
                Some((path, link))
            })
            .collect::<Vec<_>>();

        // Insert everything in the change log
        let mut change_log = ChangeLog::new();
        for (path, link) in pulled_items.iter() {
            change_log.insert(path.clone(), (*link.cid(), ChangeType::Base));
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
                (Some((pi_path, pi_link)), Some((ci_tree, ci_path))) => {
                    // First check if ci is a dir, since we skip those
                    if ci_tree.is_dir() {
                        ci_next = ci_iter.next();
                        continue;
                    }
                    if pi_path < &ci_path {
                        to_pull.push((pi_path, pi_link.cid()));
                    } else if pi_path > &ci_path {
                        to_prune.push(ci_path);
                    } else if file_needs_pull(&local_ipfs_rpc, &ci_path, pi_link.cid()).await?
                        && *pi_link.cid() != Cid::default()
                    {
                        to_pull.push((pi_path, pi_link.cid()));
                    }
                    pi_next = pi_iter.next();
                    ci_next = ci_iter.next();
                }
                (Some(pi), None) => {
                    to_pull.push((&pi.0, pi.1.cid()));
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

        // First pass - write schema files
        for (path, schema) in schemas {
            let schema_file = path.join(".schema");
            fs::create_dir_all(&path)?;
            fs::write(&schema_file, serde_json::to_string_pretty(&schema)?)?;
        }

        // Second pass - write files and their object metadata
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

pub async fn file_needs_pull(
    ipfs_rpc: &IpfsRpc,
    path: &PathBuf,
    cid: &Cid,
) -> Result<bool, PullError> {
    if !path.exists() {
        return Ok(true);
    } else if path.is_dir() {
        return Err(PullError::PathIsDirectory(path.clone()));
    }

    let hash = utils::hash_file(path, ipfs_rpc).await?;
    if hash == *cid {
        Ok(false)
    } else {
        Ok(true)
    }
}

async fn pull_file(mount: &Mount, path: &PathBuf) -> Result<(), PullError> {
    // Get the node link at this path to check if it has object metadata
    let abs_path = PathBuf::from("/").join(path);
    let parent_path = abs_path.parent()
        .ok_or_else(|| PullError::Default(anyhow::anyhow!("File has no parent path")))?;
    let file_name = path.file_name()
        .ok_or_else(|| PullError::Default(anyhow::anyhow!("Invalid file name")))?;

    // Get the parent directory's links to find our file
    let (links, _) = mount.ls(parent_path, false).await?;
    let node_link = links.iter()
        .find(|(p, _)| *p == &PathBuf::from(file_name))
        .map(|(_, link)| link.clone())
        .ok_or_else(|| PullError::Default(anyhow::anyhow!("File not found")))?;

    // Create parent directory
    let mut object_path = path.clone();
    object_path.pop();
    fs::create_dir_all(&object_path)?;

    // If this is a data link with object metadata, write it to .obj/
    if let NodeLink::Data(_, Some(object)) = node_link {
        // Create .obj directory next to file
        let obj_dir = object_path.join(".obj");
        fs::create_dir_all(&obj_dir)?;
        
        // Write object to .name.json in .obj directory
        let file_name = file_name.to_str()
            .ok_or_else(|| PullError::Default(anyhow::anyhow!("Invalid file name encoding")))?;
        let obj_file = obj_dir.join(format!(".{}.json", file_name));
        fs::write(&obj_file, serde_json::to_string_pretty(&object)?)?;
    }

    // Pull the actual file data
    let data_vec = mount.cat(&abs_path).await?;
    let mut file = fs::File::create(path)?;
    file.write_all(data_vec.as_slice())?;

    Ok(())
}

fn rm_file(path: &PathBuf) -> Result<(), PullError> {
    std::fs::remove_file(path)?;
    Ok(())
}
