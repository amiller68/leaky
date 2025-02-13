use std::convert::TryFrom;
use std::path::Path;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use url::Url;

use leaky_common::prelude::*;
use thumbs_up::prelude::{EcKey, PrivateKey};

use crate::args::Command;

use super::Args;
use super::ChangeLog;

pub const DEFAULT_CONFIG_NAME: &str = "leaky.conf";
// pub const DEFAULT_BLOCK_CACHE_NAME: &str = "leaky.cache";
pub const DEFAULT_STATE_NAME: &str = "leaky.state";
pub const DEFAULT_PREVIOUS_CID_NAME: &str = "leaky.previous_cid";
pub const DEFAULT_CHAGE_LOG_NAME: &str = "leaky.log";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OnDiskConfig {
    pub remote: Url,
    pub key_path: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OnDiskState {
    pub cid: Cid,
    pub manifest: Manifest,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreviousCid {
    pub cid: Cid,
}

pub struct AppState {
    pub path: PathBuf,
    pub on_disk_config: OnDiskConfig,
    pub on_disk_state: OnDiskState,
    pub previous_cid: PreviousCid,
    pub change_log: ChangeLog,
}

impl TryFrom<&Args> for AppState {
    type Error = AppStateSetupError;

    fn try_from(args: &Args) -> Result<Self, Self::Error> {
        let path = args.leaky_path.clone();
        let load_result = AppState::load_on_disk_config(&path);
        let load = match load_result {
            Ok((config, state, change_log, previous_cid)) => {
                Ok((config, state, change_log, previous_cid))
            }
            Err(AppStateSetupError::MissingDataPath) => match &args.command {
                Command::Init(op) => {
                    let remote = op.remote.clone();
                    let key_path = op.key_path.clone();

                    AppState::init_on_disk_config(&path, remote, key_path)?;
                    AppState::load_on_disk_config(&path)
                }
                _ => Err(AppStateSetupError::MissingDataPath),
            },
            Err(e) => Err(e),
        }?;
        let (on_disk_config, on_disk_state, change_log, previous_cid) = load;
        Ok(Self {
            path,
            on_disk_config,
            on_disk_state,
            change_log,
            previous_cid,
        })
    }
}

#[derive(Debug, thiserror::Error)]
pub enum AppStateSetupError {
    #[error("default: {0}")]
    Default(#[from] anyhow::Error),
    #[error("io: {0:?} path: {1:?}")]
    Io(std::io::Error, PathBuf),
    #[error("serde: {0}")]
    Serde(#[from] serde_json::Error),
    #[error("missing data path")]
    MissingDataPath,
    #[error("api error: {0}")]
    ApiError(#[from] leaky_common::error::ApiError),
    #[error("thumbs up error: {0}")]
    ThumbsUp(#[from] thumbs_up::prelude::KeyError),
}

impl AppState {
    pub fn client(&self) -> Result<ApiClient, AppStateSetupError> {
        let remote = self.on_disk_config.remote.clone();
        let key_path = &self.on_disk_config.key_path;
        let key_bytes =
            std::fs::read(key_path).map_err(|e| AppStateSetupError::Io(e, key_path.clone()))?;
        let key = EcKey::import(&key_bytes)?;
        let mut client = ApiClient::new(remote.as_str())?;
        client.with_credentials(key);
        Ok(client)
    }

    pub fn manifest(&self) -> &Manifest {
        &self.on_disk_state.manifest
    }

    pub fn cid(&self) -> &Cid {
        &self.on_disk_state.cid
    }

    pub fn change_log(&self) -> &ChangeLog {
        &self.change_log
    }

    pub fn previous_cid(&self) -> &Cid {
        &self.previous_cid.cid
    }

    pub fn init_on_disk_config(
        path: &PathBuf,
        remote: Url,
        key_path: PathBuf,
    ) -> Result<(), AppStateSetupError> {
        if !path.exists() {
            std::fs::create_dir_all(path).map_err(|e| AppStateSetupError::Io(e, path.clone()))?;
        }

        let config_path = path.join(PathBuf::from(DEFAULT_CONFIG_NAME));
        let state_path = path.join(PathBuf::from(DEFAULT_STATE_NAME));
        let previous_cid_path = path.join(PathBuf::from(DEFAULT_PREVIOUS_CID_NAME));
        let change_log_path = path.join(PathBuf::from(DEFAULT_CHAGE_LOG_NAME));
        let key_path = key_path.join("leaky.prv");

        // Summarize the state
        let on_disk_config = OnDiskConfig { remote, key_path };
        let on_disk_state = OnDiskState {
            cid: Cid::default(),
            manifest: Manifest::default(),
        };
        let previous_cid = PreviousCid {
            cid: Cid::default(),
        };

        // Write everything to disk
        let config_json = serde_json::to_string(&on_disk_config)?;
        let change_log_json = serde_json::to_string(&ChangeLog::new())?;
        let state_json = serde_json::to_string(&on_disk_state)?;
        let previous_cid_json = serde_json::to_string(&previous_cid)?;

        std::fs::write(&config_path, config_json)
            .map_err(|e| AppStateSetupError::Io(e, config_path))?;
        std::fs::write(&change_log_path, change_log_json)
            .map_err(|e| AppStateSetupError::Io(e, change_log_path))?;
        std::fs::write(&state_path, state_json)
            .map_err(|e| AppStateSetupError::Io(e, state_path))?;
        std::fs::write(&previous_cid_path, previous_cid_json)
            .map_err(|e| AppStateSetupError::Io(e, previous_cid_path))?;

        Ok(())
    }

    pub fn load_on_disk_config(
        path: &Path,
    ) -> Result<(OnDiskConfig, OnDiskState, ChangeLog, PreviousCid), AppStateSetupError> {
        if !path.exists() {
            return Err(AppStateSetupError::MissingDataPath);
        }

        let config_path = path.join(PathBuf::from(DEFAULT_CONFIG_NAME));
        let state_path = path.join(PathBuf::from(DEFAULT_STATE_NAME));
        let previous_cid_path = path.join(PathBuf::from(DEFAULT_PREVIOUS_CID_NAME));
        let change_log_path = path.join(PathBuf::from(DEFAULT_CHAGE_LOG_NAME));

        let config_str = std::fs::read_to_string(&config_path)
            .map_err(|e| AppStateSetupError::Io(e, config_path))?;
        let state_str = std::fs::read_to_string(&state_path)
            .map_err(|e| AppStateSetupError::Io(e, state_path))?;
        let previous_cid_str = std::fs::read_to_string(&previous_cid_path)
            .map_err(|e| AppStateSetupError::Io(e, previous_cid_path))?;
        let change_log_str = std::fs::read_to_string(&change_log_path)
            .map_err(|e| AppStateSetupError::Io(e, change_log_path))?;

        let config: OnDiskConfig = serde_json::from_str(&config_str)?;
        let state: OnDiskState = serde_json::from_str(&state_str)?;
        let previous_cid: PreviousCid = serde_json::from_str(&previous_cid_str)?;
        let change_log: ChangeLog = serde_json::from_str(&change_log_str)?;

        Ok((config, state, change_log, previous_cid))
    }

    pub fn save(
        &self,
        mount: &Mount,
        change_log: Option<&ChangeLog>,
        previous_cid: Option<Cid>,
    ) -> Result<(), AppStateSetupError> {
        let path = &self.path;
        if !path.exists() {
            return Err(AppStateSetupError::MissingDataPath);
        }

        let state_path = path.join(PathBuf::from(DEFAULT_STATE_NAME));
        let change_log_path = path.join(PathBuf::from(DEFAULT_CHAGE_LOG_NAME));

        let cid = *mount.cid();
        let manifest = mount.manifest();

        let on_disk_state = OnDiskState { cid, manifest };
        let state_json = serde_json::to_string(&on_disk_state)?;
        std::fs::write(&state_path, state_json)
            .map_err(|e| AppStateSetupError::Io(e, state_path))?;

        if let Some(cid) = previous_cid {
            let previous_cid_path = path.join(PathBuf::from(DEFAULT_PREVIOUS_CID_NAME));
            let previous_cid = PreviousCid { cid };
            let previous_cid_json = serde_json::to_string(&previous_cid)?;
            std::fs::write(&previous_cid_path, previous_cid_json)
                .map_err(|e| AppStateSetupError::Io(e, previous_cid_path))?;
        }

        if let Some(change_log) = change_log {
            let change_log_json = serde_json::to_string(change_log)?;
            std::fs::write(&change_log_path, change_log_json)
                .map_err(|e| AppStateSetupError::Io(e, change_log_path))?;
        }

        Ok(())
    }
}
