use std::path::PathBuf;

use anyhow::Result;
use leaky_common::prelude::*;

use fs_tree::FsTree;

pub const DEFAULT_LOCAL_DIR: &str = ".leaky";
pub const DEFAULT_CONFIG_NAME: &str = "leaky.conf";
pub const DEFAULT_STATE_NAME: &str = "leaky.state";
pub const DEFAULT_CHAGE_LOG_NAME: &str = "leaky.log";

pub fn fs_tree() -> Result<FsTree> {
    let dot_dir = PathBuf::from(DEFAULT_LOCAL_DIR);

    // Read the Fs-tree at the local directory, ignoring the local directory
    // Read Fs-tree at dir or pwd, stripping off the local dot directory
    match fs_tree::FsTree::read_at(".")? {
        FsTree::Directory(mut d) => {
            let _res = &d.remove_entry(&dot_dir);
            Ok(fs_tree::FsTree::Directory(d))
        }
        _ => Err(anyhow::anyhow!("Expected a directory")),
    }
}

pub async fn hash_file(path: &PathBuf, ipfs: &IpfsRpc) -> Result<Cid> {
    if !path.exists() {
        return Err(anyhow::anyhow!("File does not exist"));
    }
    if !path.is_file() {
        return Err(anyhow::anyhow!("Expected a file"));
    }

    let file = std::fs::File::open(path)?;

    let cid = ipfs.hash_data(file).await?;

    Ok(cid)
}
