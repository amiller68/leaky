use parking_lot::Mutex;
use std::collections::BTreeMap;
use std::convert::TryFrom;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use axum::extract::FromRef;
use url::Url;

use leaky_common::prelude::*;

use super::config::Config;
use crate::database::{models::RootCid, Database};

#[derive(Clone)]
pub struct AppState {
    get_content_forwarding_url: Url,
    sqlite_database: Database,
    mount: Arc<Mutex<Mount>>,
}

#[allow(dead_code)]
impl AppState {
    pub fn get_content_forwarding_url(&self) -> &Url {
        &self.get_content_forwarding_url
    }

    pub fn sqlite_database(&self) -> &Database {
        &self.sqlite_database
    }

    pub fn mount(&self) -> Arc<Mutex<Mount>> {
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
                RootCid::push(&cid, &previous_cid, &mut conn).await?;
                mount
            }
        };

        Ok(Self {
            get_content_forwarding_url: config.get_content_forwarding_url().clone(),
            sqlite_database,
            mount: Arc::new(Mutex::new(mount)),
        })
    }

    pub fn mount_guard(&self) -> MountGuard {
        let guard = unsafe {
            std::mem::transmute::<
                parking_lot::MutexGuard<'_, Mount>,
                parking_lot::MutexGuard<'static, Mount>,
            >(self.mount.lock())
        };
        MountGuard { _lock: guard }
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

impl FromRef<AppState> for Arc<Mutex<Mount>> {
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

pub struct MountGuard {
    _lock: parking_lot::MutexGuard<'static, Mount>,
}

// Explicitly implement Send for MountGuard
// SAFETY: Mount is Send, and we're using parking_lot::Mutex which is Send
unsafe impl Send for MountGuard {}

impl MountGuard {
    pub fn cid(&self) -> &Cid {
        self._lock.cid()
    }

    pub async fn ls(
        &self,
        path: &Path,
    ) -> Result<(BTreeMap<PathBuf, NodeLink>, Option<Schema>), MountError> {
        self._lock.ls(path).await
    }

    pub async fn ls_deep(
        &self,
        path: &Path,
    ) -> Result<(BTreeMap<PathBuf, NodeLink>, BTreeMap<PathBuf, Schema>), MountError> {
        self._lock.ls_deep(path).await
    }

    pub async fn cat(&self, path: &Path) -> Result<Vec<u8>, MountError> {
        self._lock.cat(path).await
    }

    pub async fn update(&mut self, cid: Cid) -> Result<(), MountError> {
        self._lock.update(cid).await
    }

    pub fn manifest(&self) -> Manifest {
        self._lock.manifest()
    }
}
