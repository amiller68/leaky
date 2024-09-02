use std::convert::TryFrom;

use axum::extract::FromRef;

use leaky_common::prelude::*;

use super::config::Config;
use crate::database::Database;

#[derive(Clone)]
pub struct AppState {
    sqlite_database: Database,
    ipfs_rpc: IpfsRpc<IpfsClient>,
}

#[allow(dead_code)]
impl AppState {
    pub fn sqlite_database(&self) -> &Database {
        &self.sqlite_database
    }

    pub fn ipfs_rpc(&self) -> &IpfsRpc<IpfsClient> {
        &self.ipfs_rpc
    }

    pub async fn from_config(config: &Config) -> Result<Self, AppStateSetupError> {
        let sqlite_database = Database::connect(config.sqlite_database_url()).await?;
        let ipfs_rpc = IpfsRpc::try_from(config.ipfs_rpc_url().clone())?;

        Ok(Self {
            sqlite_database,
            ipfs_rpc,
        })
    }
}

impl FromRef<AppState> for Database {
    fn from_ref(app_state: &AppState) -> Self {
        app_state.sqlite_database.clone()
    }
}

impl FromRef<AppState> for IpfsRpc<IpfsClient> {
    fn from_ref(app_state: &AppState) -> Self {
        app_state.ipfs_rpc.clone()
    }
}

#[derive(Debug, thiserror::Error)]
pub enum AppStateSetupError {
    #[error("failed to setup the database: {0}")]
    DatabaseSetup(#[from] crate::database::DatabaseSetupError),
    #[error("failed to setup the IPFS RPC client: {0}")]
    IpfsRpcError(#[from] leaky_common::error::IpfsRpcError),
}
