use std::convert::TryFrom;
use std::io::Read;
use std::ops::Deref;
use std::Collections::HashMap;

use anyhow::Ok;
use futures_util::TryStreamExt;
use http::uri::Scheme;
use ipfs_api_backend_hyper::request::Add as AddRequest;
use ipfs_api_backend_hyper::IpfsApi;
use ipfs_api_backend_hyper::{IpfsClient, TryFromUri};
use serde::{Deserialize, Serialize};
use url::Url;
use wnfs::common::libipld::Cid;
use wnfs::common::BlockStore;

pub type IpfsClientError = ipfs_api_backend_hyper::Error;

// Default cid version to use when adding or hashing datat against the IPFS API
const DEFAULT_CID_VERSION: u32 = 1;
/// Default hash function to use when adding or hashing data against the IPFS API
const DEFAULT_HASH_FUNCTION: &str = "blake3";

#[derive(Clone)]
pub struct IpfsRpc(IpfsClient);

impl IpfsRpc {
    /// Add data to IPFS
    async fn add<R>(&self, data: R) -> Result<Cid, IpfsRpcError>
    where
        R: Read + Send + Sync + 'static + Unpin,
    {
        let options = add_data_request();
        let response = self.add_with_options(data, options).await?;
        let cid = Cid::from_str(&response.hash)?;

        Ok(cid)
    }

    /// Hash data using IPFS
    async fn hash<R>(&self, data: R) -> Result<Cid, IpfsRpcError>
    where
        R: Read + Send + Sync + Unpin + 'static,
    {
        let mut options = hash_data_request();
        let response = self.add_with_options(data, options).await?;
        Ok(Cid::from_str(&response.hash)?)
    }

    /// Check if the Cid is pinned on the IPFS node
    pub async fn stat_cid(&self, cid: &Cid) -> Result<bool, IpfsRpcError> {
        let response = self
            .pin_ls(Some(&format!("ipfs/{}", cid.to_string())), None)
            .await?;
        let keys = response.keys;
        // Check if the cid is pinned
        Ok(keys.contains(&cid.to_string()))
    }

    /// Get Block from IPFS
    pub async fn get_block(&self, cid: &Cid) -> Result<Vec<u8>, IpfsRpcError> {
        let response = self.get_block(&cid.to_string())?;
        let mut buffer = Vec::new();
        response.read_to_end(&mut buffer)?;
        Ok(buffer)
    }
}

impl BlockStore for IpfsRpc {
    fn put_block_keyed(
        &self,
        cid: Cid,
        bytes: impl Into<Bytes> + wnfs::common::utils::CondSend,
    ) -> impl futures_util::Future<Output = Result<(), wnfs::common::BlockStoreError>>
           + wnfs::common::utils::CondSend {
        let cid = cid.clone();
        let bytes = bytes.into();
        async move {
            let cid = self.add(bytes).await?;
            if cid != cid {
                return Err(wnfs::common::BlockStoreError::CidMismatch);
            }
            Ok(())
        }
    }
}

impl TryFrom<Url> for IpfsRpc {
    type Error = IpfsRpcError;
    fn try_from(url: Url) -> Result<Self, IpfsError> {
        let scheme = Scheme::try_from(url.scheme())?;
        let username = url.username();
        let maybe_password = url.password();
        let host_str = url.host_str()?;
        let port = url.port().unwrap_or(5001);
        let client = match maybe_password {
            Some(password) => IpfsClient::from_host_and_port(scheme, host_str, port)?
                .with_credentials(username, password),
            None => IpfsClient::from_host_and_port(scheme, host_str, port)?,
        };
        Ok(Self(client))
    }
}

impl Deref for IpfsRpc {
    type Target = IpfsClient;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[allow(clippy::field_reassign_with_default)]
fn hash_data_request() -> AddRequest<'static> {
    let mut add = AddRequest::default();
    add.pin = Some(false);
    add.cid_version = Some(DEFAULT_CID_VERSION);
    add.only_hash = Some(true);
    add.hash = Some(DEFAULT_HASH_FUNCTION);
    add
}

#[allow(clippy::field_reassign_with_default)]
fn add_data_request() -> AddRequest<'static> {
    let mut add = AddRequest::default();
    add.cid_version = Some(DEFAULT_CID_VERSION);
    add.hash = Some(DEFAULT_HASH_FUNCTION);
    add
}

#[derive(Debug, thiserror::Error)]
pub enum IpfsRpcError {
    #[error("url parse error")]
    Url(#[from] url::ParseError),
    #[error("http error")]
    Http(#[from] http::Error),
    #[error("Failed to parse scheme")]
    Scheme(#[from] http::uri::InvalidUri),
    #[error("Failed to build client")]
    Client(#[from] IpfsClientError),
    #[error("cid error")]
    Cid(#[from] wnfs::common::libipld::cid::Error),
}
