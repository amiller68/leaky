use std::convert::TryFrom;

use axum::extract::FromRef;
use url::Url;

use leaky_common::prelude::*;

use super::config::Config;
use crate::database::{models::RootCid, Database};

#[derive(Clone)]
pub struct AppState {
    get_content_forwarding_url: Url,
    sqlite_database: Database,
    mount: Mount,
}

#[allow(dead_code)]
impl AppState {
    pub fn get_content_forwarding_url(&self) -> &Url {
        &self.get_content_forwarding_url
    }

    pub fn sqlite_database(&self) -> &Database {
        &self.sqlite_database
    }

    pub fn mount(&self) -> Mount {
        self.mount.clone()
    }

    pub async fn from_config(config: &Config) -> Result<Self, AppStateSetupError> {
        let sqlite_database = Database::connect(config.sqlite_database_url()).await?;
        let ipfs_rpc = IpfsRpc::try_from(config.ipfs_rpc_url().clone())?;
        let mut conn = sqlite_database.acquire().await?;
        let maybe_root_cid = RootCid::pull(&mut conn).await?;
        let mount = match maybe_root_cid {
            Some(rc) => Mount::pull(rc.cid(), &ipfs_rpc).await?,

            None => {
                let mount = Mount::init(&ipfs_rpc).await?;
                let previous_cid = mount.previous_cid();
                let cid = mount.cid();
                // set the root cid
                RootCid::push(&cid, &previous_cid, &mut conn).await?;
                mount
            }
        };

        Ok(Self {
            get_content_forwarding_url: config.get_content_forwarding_url().clone(),
            sqlite_database,
            // ipfs_rpc,
            mount,
        })
    }
}

impl FromRef<AppState> for Database {
    fn from_ref(app_state: &AppState) -> Self {
        app_state.sqlite_database.clone()
    }
}

// impl FromRef<AppState> for IpfsRpc {
//     fn from_ref(app_state: &AppState) -> Self {
//         app_state.ipfs_rpc.clone()
//     }
// }

impl FromRef<AppState> for Mount {
    fn from_ref(app_state: &AppState) -> Self {
        app_state.mount.clone()
    }
}

#[derive(Debug, thiserror::Error)]
pub enum AppStateSetupError {
    #[error("failed to setup the database: {0}")]
    DatabaseSetup(#[from] crate::database::DatabaseSetupError),
    #[error("sqlx: {0}")]
    Database(#[from] sqlx::Error),
    #[error("failed to setup the IPFS RPC client: {0}")]
    IpfsRpcError(#[from] leaky_common::error::IpfsRpcError),
    #[error("root CID error: {0}")]
    RootCid(#[from] crate::database::models::RootCidError),
    #[error("mount error: {0}")]
    Mount(#[from] MountError),
    #[error("Unsupported image format")]
    UnsupportedImageFormat,
}
