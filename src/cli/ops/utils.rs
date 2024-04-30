use std::io::{Read, Write};
use std::path::PathBuf;

use anyhow::Result;
use leaky::prelude::*;

use fs_tree::FsTree;
use serde::{Deserialize, Serialize};
use url::Url;

use super::change_log::ChangeLog;

pub const DEFAULT_LOCAL_LEAKY_DIR: &str = ".leaky";
pub const DEFAULT_LEAKY_NAME: &str = "leaky.json";
pub const DEFAULT_CHAGE_LOG_NAME: &str = "leaky.log";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LeakyConfig {
    pub cid: Cid,
    pub ipfs_rpc: Url,
}

pub fn init_leaky_config(ipfs_rpc: Url, cid: Cid) -> Result<()> {
    let path = PathBuf::from(&format!(
        "{}/{}",
        DEFAULT_LOCAL_LEAKY_DIR, DEFAULT_LEAKY_NAME
    ));
    let config = LeakyConfig { cid, ipfs_rpc };
    // Check if the config file already exists
    if path.exists() {
        return Err(anyhow::anyhow!("Config file already exists"));
    }
    // make sure the directory exists
    std::fs::create_dir_all(&path.parent().unwrap())?;
    let mut file = std::fs::File::create(&path)?;
    file.write_all(serde_json::to_string(&config)?.as_bytes())?;
    Ok(())
}

pub async fn load_leaky() -> Result<Leaky> {
    let path = PathBuf::from(&format!(
        "{}/{}",
        DEFAULT_LOCAL_LEAKY_DIR, DEFAULT_LEAKY_NAME
    ));

    if !path.exists() {
        return Err(anyhow::anyhow!("Config file does not exist"));
    }
    let config = std::fs::read_to_string(path)?;
    let leaky_config: LeakyConfig = serde_json::from_str(&config)?;

    let mut leaky = Leaky::new(leaky_config.ipfs_rpc)?;
    leaky.pull(&leaky_config.cid).await?;
    Ok(leaky)
}

pub async fn update_leaky(leaky: &mut Leaky) -> Result<()> {
    let path = PathBuf::from(&format!(
        "{}/{}",
        DEFAULT_LOCAL_LEAKY_DIR, DEFAULT_LEAKY_NAME
    ));
    if !path.exists() {
        return Err(anyhow::anyhow!("Config file does not exist"));
    }
    leaky.push().await?;
    let cid = leaky.cid()?;
    let config_str = std::fs::read_to_string(path.clone())?;
    let config: LeakyConfig = serde_json::from_str(&config_str)?;
    let new_config = LeakyConfig {
        cid,
        ipfs_rpc: config.ipfs_rpc,
    };
    let mut file = std::fs::OpenOptions::new().write(true).open(path.clone())?;
    file.write_all(serde_json::to_string(&new_config)?.as_bytes())?;
    Ok(())
}

pub fn save_change_log(log: &ChangeLog) -> Result<()> {
    let path = PathBuf::from(&format!(
        "{}/{}",
        DEFAULT_LOCAL_LEAKY_DIR, DEFAULT_CHAGE_LOG_NAME
    ));
    let log = serde_json::to_string(&log)?;
    let mut file = std::fs::OpenOptions::new().create(true).open(path)?;
    file.write_all(serde_json::to_string(&log)?.as_bytes())?;
    Ok(())
}

pub fn load_change_log() -> Result<ChangeLog> {
    let path = PathBuf::from(&format!(
        "{}/{}",
        DEFAULT_LOCAL_LEAKY_DIR, DEFAULT_CHAGE_LOG_NAME
    ));
    if !path.exists() {
        return Ok(ChangeLog::new());
    }
    let logs = std::fs::read_to_string(path)?;
    let logs: ChangeLog = serde_json::from_str(&logs)?;
    Ok(logs)
}

pub fn fs_tree() -> Result<FsTree> {
    let dot_dir = PathBuf::from(DEFAULT_LOCAL_LEAKY_DIR);
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

pub fn hash_file(path: &PathBuf) -> Result<blake3::Hash> {
    if !path.exists() {
        return Err(anyhow::anyhow!("File does not exist"));
    }
    if !path.is_file() {
        return Err(anyhow::anyhow!("Expected a file"));
    }

    let mut file = std::fs::File::open(path)?;
    let mut hasher = blake3::Hasher::new();
    let buffer = &mut [0; 1024];
    while let Ok(bytes_read) = file.read(buffer) {
        if bytes_read == 0 {
            break;
        }
        hasher.update(&buffer[..bytes_read]);
    }
    Ok(hasher.finalize())
}
