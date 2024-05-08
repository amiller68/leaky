use std::collections::HashMap;
use std::path::PathBuf;

use anyhow::Result;
use leaky::prelude::*;

use fs_tree::FsTree;
use serde::{Deserialize, Serialize};
use url::Url;

use super::change_log::ChangeLog;

pub const DEFAULT_LOCAL_DIR: &str = ".leaky";
pub const DEFAULT_CONFIG_NAME: &str = "leaky.conf";
pub const DEFAULT_CACHE_NAME: &str = "leaky.cache";
pub const DEFAULT_STATE_NAME: &str = "leaky.state";
pub const DEFAULT_CHAGE_LOG_NAME: &str = "leaky.log";

fn ser_cid(cid: &Cid) -> String {
    format!("cid-{}", cid)
}

fn deser_cid(cid: &str) -> Cid {
    let cid = &cid[4..];
    Cid::try_from(cid.to_string()).unwrap()
}

fn ser_ipld(ipld: &Ipld) -> ipld_core::ipld::Ipld {
    match ipld {
        Ipld::Null => ipld_core::ipld::Ipld::Null,
        Ipld::Bool(b) => ipld_core::ipld::Ipld::Bool(*b),
        Ipld::Integer(i) => ipld_core::ipld::Ipld::Integer(*i),
        Ipld::Float(f) => ipld_core::ipld::Ipld::Float(*f),
        Ipld::Bytes(b) => ipld_core::ipld::Ipld::Bytes(b.clone()),
        Ipld::String(s) => ipld_core::ipld::Ipld::String(s.clone()),
        Ipld::List(l) => {
            let l = l.iter().map(|i| ser_ipld(i)).collect();
            ipld_core::ipld::Ipld::List(l)
        }
        Ipld::Map(m) => {
            let m = m.iter().map(|(k, v)| {
                // Key must be a string
                let k = k.clone();
                let v = ser_ipld(v);
                (k, v)
            });
            let m = m.collect();
            ipld_core::ipld::Ipld::Map(m)
        }
        // TODO: UPSER JANK BUT WORKS
        Ipld::Link(cid) => ipld_core::ipld::Ipld::String(ser_cid(cid)),
    }
}

fn deser_ipld(ipld: &ipld_core::ipld::Ipld) -> Ipld {
    match ipld {
        ipld_core::ipld::Ipld::Null => Ipld::Null,
        ipld_core::ipld::Ipld::Bool(b) => Ipld::Bool(*b),
        ipld_core::ipld::Ipld::Integer(i) => Ipld::Integer(*i),
        ipld_core::ipld::Ipld::Float(f) => Ipld::Float(*f),
        ipld_core::ipld::Ipld::Bytes(b) => Ipld::Bytes(b.clone()),
        // JANK as hell
        ipld_core::ipld::Ipld::String(s) => {
            // Check if it is a link
            if s.starts_with("cid-") {
                Ipld::Link(deser_cid(s))
            } else {
                Ipld::String(s.clone())
            }
        }
        ipld_core::ipld::Ipld::List(l) => {
            let l = l.iter().map(|i| deser_ipld(i)).collect();
            Ipld::List(l)
        }
        ipld_core::ipld::Ipld::Map(m) => {
            let m = m.iter().map(|(k, v)| {
                // Key must be a string
                let k = k.clone();
                let v = deser_ipld(v);
                (k, v)
            });
            let m = m.collect();
            Ipld::Map(m)
        }
        // TODO: shouldnt ever happen
        // ipld_core::ipld::Ipld::Link(cid) => Ipld::Link(Cid::try_from(cid.to_string()).unwrap()),
        _ => panic!("Unexpected Ipld type"),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OnDiskConfig {
    pub ipfs_rpc_url: Url,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OnDiskState {
    pub cid: Cid,
    pub manifest: Manifest,
}

pub async fn init_on_disk(ipfs_rpc_url: Url, cid: Option<Cid>) -> Result<Leaky> {
    let local_dir_path = PathBuf::from(DEFAULT_LOCAL_DIR);
    let config_path = local_dir_path.join(PathBuf::from(DEFAULT_CONFIG_NAME));
    let state_path = local_dir_path.join(PathBuf::from(DEFAULT_STATE_NAME));
    let cache_path = local_dir_path.join(PathBuf::from(DEFAULT_CACHE_NAME));
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

    // Get the initial state
    let cid = leaky.cid()?;
    let manifest = leaky.manifest()?;

    // Get the init the cache and serialize
    let block_cache = leaky.block_cache()?;
    let ser_block_cache: HashMap<String, ipld_core::ipld::Ipld> = block_cache
        .iter()
        .map(|(k, v)| (k.clone(), ser_ipld(v)))
        .collect();

    // Summarize the state
    let on_disk_config = OnDiskConfig { ipfs_rpc_url };
    let on_disk_state = OnDiskState { cid, manifest };

    // Write everything to disk
    std::fs::create_dir_all(&local_dir_path)?;
    std::fs::write(config_path, serde_json::to_string(&on_disk_config)?)?;
    std::fs::write(state_path, serde_json::to_string(&on_disk_state)?)?;
    let cache_file = std::fs::File::create(cache_path)?;
    serde_ipld_dagcbor::to_writer(cache_file, &ser_block_cache)?;
    std::fs::write(change_log_path, serde_json::to_string(&ChangeLog::new())?)?;

    Ok(leaky)
}

pub async fn load_on_disk() -> Result<(Leaky, ChangeLog)> {
    let local_dir_path = PathBuf::from(DEFAULT_LOCAL_DIR);
    let config_path = local_dir_path.join(PathBuf::from(DEFAULT_CONFIG_NAME));
    let state_path = local_dir_path.join(PathBuf::from(DEFAULT_STATE_NAME));
    let cache_path = local_dir_path.join(PathBuf::from(DEFAULT_CACHE_NAME));
    let change_log_path = local_dir_path.join(PathBuf::from(DEFAULT_CHAGE_LOG_NAME));

    if !local_dir_path.exists() {
        return Err(anyhow::anyhow!("No leaky directory found"));
    }

    use std::io::BufReader;

    let config_str = std::fs::read_to_string(config_path)?;
    let config: OnDiskConfig = serde_json::from_str(&config_str)?;
    let state_str = std::fs::read_to_string(state_path)?;
    let state: OnDiskState = serde_json::from_str(&state_str)?;
    let cache_file = std::fs::File::open(cache_path)?;
    let cache_reader = BufReader::new(cache_file);
    let ser_block_cache: HashMap<String, ipld_core::ipld::Ipld> =
        serde_ipld_dagcbor::from_reader(cache_reader)?;

    let block_cache: HashMap<String, Ipld> = ser_block_cache
        .iter()
        .map(|(k, v)| (k.clone(), deser_ipld(v)))
        .collect();
    let block_cache: BlockCache = BlockCache(block_cache);

    let mut leaky = Leaky::new(config.ipfs_rpc_url)?;
    leaky.load(&state.cid, &state.manifest, block_cache).await?;

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
    let cache_path = local_dir_path.join(PathBuf::from(DEFAULT_CACHE_NAME));
    let change_log_path = local_dir_path.join(PathBuf::from(DEFAULT_CHAGE_LOG_NAME));

    if !local_dir_path.exists() {
        return Err(anyhow::anyhow!("No leaky directory found"));
    }

    let cid = leaky.cid()?;
    let manifest = leaky.manifest()?;
    let block_cache = leaky.block_cache()?;

    // Iterate over the block cache and ser_ipld
    let ser_block_cache: HashMap<String, ipld_core::ipld::Ipld> = block_cache
        .iter()
        .map(|(k, v)| (k.clone(), ser_ipld(v)))
        .collect();

    let on_disk_state = OnDiskState { cid, manifest };

    std::fs::write(state_path, serde_json::to_string(&on_disk_state)?)?;
    let cache_file = std::fs::OpenOptions::new()
        .write(true)
        .truncate(true)
        .open(cache_path)?;
    serde_ipld_dagcbor::to_writer(cache_file, &ser_block_cache)?;
    std::fs::write(change_log_path, serde_json::to_string(&change_log)?)?;

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
