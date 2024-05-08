use std::path::PathBuf;

use anyhow::Result;
use leaky::prelude::*;

use fs_tree::FsTree;
use serde::{Deserialize, Serialize};
use url::Url;

use super::change_log::ChangeLog;

pub const DEFAULT_LOCAL_DIR: &str = ".leaky";
pub const DEFAULT_CONFIG_NAME: &str = "leaky.conf";
pub const DEFAULT_STATE_NAME: &str = "leaky.state";
pub const DEFAULT_CHAGE_LOG_NAME: &str = "leaky.log";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OnDiskConfig {
    pub ipfs_rpc_url: Url,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OnDiskState {
    pub cid: Cid,
    pub manifest: Manifest,
    pub block_cache: BlockCache,
}

pub async fn init_on_disk(ipfs_rpc_url: Url, cid: Option<Cid>) -> Result<Leaky> {
    let local_dir_path = PathBuf::from(DEFAULT_LOCAL_DIR);
    let config_path = local_dir_path.join(PathBuf::from(DEFAULT_CONFIG_NAME));
    let state_path = local_dir_path.join(PathBuf::from(DEFAULT_STATE_NAME));
    let change_log_path = local_dir_path.join(PathBuf::from(DEFAULT_CHAGE_LOG_NAME));

    // Check whether the dir exists
    if local_dir_path.exists() {
        return Err(anyhow::anyhow!(
            "Local directory already exists at {:?}",
            local_dir_path
        ));
    }

    // Initialize Leaky
    let mut leaky = Leaky::new(ipfs_rpc_url.clone())?;

    if let Some(cid) = cid {
        leaky.pull(&cid).await?;
    } else {
        leaky.init().await?;
    }

    let cid = leaky.cid()?;
    let block_cache = leaky.block_cache()?;
    let manifest = leaky.manifest()?;

    let on_disk_config = OnDiskConfig { ipfs_rpc_url };

    let on_disk_state = OnDiskState {
        cid,
        manifest,
        block_cache,
    };

    // Write config to disk

    std::fs::create_dir_all(&local_dir_path)?;
    std::fs::write(config_path, serde_json::to_string(&on_disk_config)?)?;
    std::fs::write(state_path, serde_json::to_string(&on_disk_state)?)?;
    std::fs::write(change_log_path, serde_json::to_string(&ChangeLog::new())?)?;

    Ok(leaky)
}

pub async fn load_on_disk() -> Result<(Leaky, ChangeLog)> {
    let local_dir_path = PathBuf::from(DEFAULT_LOCAL_DIR);
    let config_path = local_dir_path.join(PathBuf::from(DEFAULT_CONFIG_NAME));
    let state_path = local_dir_path.join(PathBuf::from(DEFAULT_STATE_NAME));
    let change_log_path = local_dir_path.join(PathBuf::from(DEFAULT_CHAGE_LOG_NAME));

    if !local_dir_path.exists() {
        return Err(anyhow::anyhow!("No leaky directory found"));
    }

    let config_str = std::fs::read_to_string(config_path)?;
    let config: OnDiskConfig = serde_json::from_str(&config_str)?;
    let state_str = std::fs::read_to_string(state_path)?;
    let state: OnDiskState = serde_json::from_str(&state_str)?;

    let mut leaky = Leaky::new(config.ipfs_rpc_url)?;
    leaky.load(&state.manifest, state.block_cache).await?;

    // Check if the cid in config matches the cid in the state
    let cid = leaky.cid()?;
    if cid != state.cid {
        return Err(anyhow::anyhow!("Cid in config does not match cid in state"));
    }

    let change_log_str = std::fs::read_to_string(change_log_path)?;
    let change_log: ChangeLog = serde_json::from_str(&change_log_str)?;

    Ok((leaky, change_log))
}

pub async fn save_on_disk(leaky: &mut Leaky, change_log: &ChangeLog) -> Result<()> {
    let local_dir_path = PathBuf::from(DEFAULT_LOCAL_DIR);
    let state_path = local_dir_path.join(PathBuf::from(DEFAULT_STATE_NAME));
    let change_log_path = local_dir_path.join(PathBuf::from(DEFAULT_CHAGE_LOG_NAME));

    if !local_dir_path.exists() {
        return Err(anyhow::anyhow!("No leaky directory found"));
    }

    let cid = leaky.cid()?;
    let block_cache = leaky.block_cache()?;
    let manifest = leaky.manifest()?;

    let on_disk_state = OnDiskState {
        cid,
        manifest,
        block_cache: block_cache.clone(),
    };
    println!("Block cache: {:?}", block_cache.clone());

    std::fs::write(state_path, serde_json::to_string(&on_disk_state)?)?;
    std::fs::write(change_log_path, serde_json::to_string(&change_log)?)?;

    let after_b_c = leaky.block_cache()?;

    println!("After BC: {:?}", after_b_c);

    assert_eq!(block_cache, after_b_c);

    Ok(())
}

pub fn fs_tree() -> Result<FsTree> {
    let dot_dir = PathBuf::from(DEFAULT_LOCAL_DIR);
    // Read Fs-tree at dir or pwd, stripping off the local dot directory
    let next = match fs_tree::FsTree::read_at(".")? {
        FsTree::Directory(mut d) => {
            let _res = &d.remove_entry(&dot_dir);
            fs_tree::FsTree::Directory(d)
        }
        _ => {
            return Err(anyhow::anyhow!("Expected a directory"));
        }
    };
    Ok(next)
}

pub async fn hash_file(path: &PathBuf, leaky: &Leaky) -> Result<Cid> {
    if !path.exists() {
        return Err(anyhow::anyhow!("File does not exist"));
    }
    if !path.is_file() {
        return Err(anyhow::anyhow!("Expected a file"));
    }

    let file = std::fs::File::open(path)?;

    let cid = leaky.hash_data(file).await?;

    Ok(cid)
}
