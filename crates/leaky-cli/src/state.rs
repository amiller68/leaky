use std::collections::HashMap;
use std::convert::TryFrom;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use url::Url;

use leaky_common::prelude::*;

use crate::args::Command;

use super::Args;
use super::ChangeLog;

pub const DEFAULT_CONFIG_NAME: &str = "leaky.conf";
// pub const DEFAULT_BLOCK_CACHE_NAME: &str = "leaky.cache";
pub const DEFAULT_STATE_NAME: &str = "leaky.state";
pub const DEFAULT_CHAGE_LOG_NAME: &str = "leaky.log";

#[derive(Debug, Clone, Serialize, Deserialize)]
struct OnDiskConfig {
    pub remote: Url,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OnDiskState {
    pub cid: Cid,
    pub manifest: Manifest,
}

pub struct AppState {
    pub path: PathBuf,
    pub on_disk_config: OnDiskConfig,
    pub on_disk_state: OnDiskState,
    pub change_log: ChangeLog,
}

impl TryFrom<&Args> for AppState {
    type Error = AppStateSetupError;

    fn try_from(args: &Args) -> Result<Self, Self::Error> {
        let path = args.leaky_path.clone();
        let load_result = AppState::load_on_disk_config(&path);
        let load = match load_result {
            Ok((config, state, change_log)) => Ok((config, state, change_log)),
            Err(AppStateSetupError::MissingDataPath) => match &args.command {
                Command::Init(op) => {
                    let remote = op.remote.clone();

                    AppState::init_on_disk_config(&path, remote)?;
                    AppState::load_on_disk_config(&path)
                }
                _ => Err(AppStateSetupError::MissingDataPath),
            },
            Err(e) => Err(e),
        }?;
        let (on_disk_config, on_disk_state, change_log) = load;
        Ok(Self {
            path,
            on_disk_config,
            on_disk_state,
            change_log,
        })
    }
}

#[derive(Debug, thiserror::Error)]
pub enum AppStateSetupError {
    #[error("default: {0}")]
    Default(#[from] anyhow::Error),

    #[error("io: {0}")]
    Io(#[from] std::io::Error),

    #[error("serde: {0}")]
    Serde(#[from] serde_json::Error),

    #[error("missing data path")]
    MissingDataPath,
    #[error("api error: {0}")]
    ApiError(#[from] leaky_common::error::ApiError),
}

impl AppState {
    pub fn client(&self) -> Result<ApiClient, AppStateSetupError> {
        let remote = self.on_disk_config.remote.clone();
        Ok(ApiClient::new(remote.as_str())?)
    }

    pub fn init_on_disk_config(path: &PathBuf, remote: Url) -> Result<(), AppStateSetupError> {
        if !path.exists() {
            std::fs::create_dir_all(path)?;
        }

        let config_path = path.join(PathBuf::from(DEFAULT_CONFIG_NAME));
        let state_path = path.join(PathBuf::from(DEFAULT_STATE_NAME));
        let change_log_path = path.join(PathBuf::from(DEFAULT_CHAGE_LOG_NAME));

        // Summarize the state
        let on_disk_config = OnDiskConfig { remote };
        let on_disk_state = OnDiskState {
            cid: Cid::default(),
            manifest: Manifest::default(),
        };

        // Write everything to disk
        std::fs::write(config_path, serde_json::to_string(&on_disk_config)?)?;
        std::fs::write(change_log_path, serde_json::to_string(&ChangeLog::new())?)?;
        std::fs::write(state_path, serde_json::to_string(&on_disk_state)?)?;

        Ok(())
    }

    pub fn load_on_disk_config(
        path: &PathBuf,
    ) -> Result<(OnDiskConfig, OnDiskState, ChangeLog), AppStateSetupError> {
        if !path.exists() {
            return Err(AppStateSetupError::MissingDataPath);
        }

        let config_path = path.join(PathBuf::from(DEFAULT_CONFIG_NAME));
        let state_path = path.join(PathBuf::from(DEFAULT_STATE_NAME));
        let change_log_path = path.join(PathBuf::from(DEFAULT_CHAGE_LOG_NAME));

        let config_str = std::fs::read_to_string(config_path)?;
        let config: OnDiskConfig = serde_json::from_str(&config_str)?;
        let state_str = std::fs::read_to_string(state_path)?;
        let state: OnDiskState = serde_json::from_str(&state_str)?;
        let change_log_str = std::fs::read_to_string(change_log_path)?;
        let change_log: ChangeLog = serde_json::from_str(&change_log_str)?;

        Ok((config, state, change_log))
    }

    pub fn save(
        &self,
        mount: &Mount,
        change_log: Option<&ChangeLog>,
    ) -> Result<(), AppStateSetupError> {
        let path = &self.path;
        if !path.exists() {
            return Err(AppStateSetupError::MissingDataPath);
        }

        let state_path = path.join(PathBuf::from(DEFAULT_STATE_NAME));
        let change_log_path = path.join(PathBuf::from(DEFAULT_CHAGE_LOG_NAME));

        let cid = mount.cid().clone();
        let manifest = mount.manifest();

        let on_disk_state = OnDiskState { cid, manifest };

        std::fs::write(state_path, serde_json::to_string(&on_disk_state)?)?;
        if let Some(change_log) = change_log {
            std::fs::write(change_log_path, serde_json::to_string(&change_log)?)?;
        }

        Ok(())
    }
}
