// TODO: its a dumpster fire, but it works
use std::convert::TryFrom;
use std::io::Read;
use std::ops::Deref;
use std::str::FromStr;

use bytes::Bytes;
use futures_util::TryFutureExt;
use futures_util::TryStreamExt;
use http::uri::Scheme;
use ipfs_api_backend_hyper::request::Add as AddRequest;
use ipfs_api_backend_hyper::IpfsApi;
use ipfs_api_backend_hyper::{IpfsClient, TryFromUri};
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
    pub async fn add<R>(&self, data: R) -> Result<Cid, IpfsRpcError>
    where
        R: Read + Send + Sync + 'static + Unpin,
    {
        let options = add_data_request();
        let response = self.add_with_options(data, options).await?;
        let cid = Cid::from_str(&response.hash)?;

        Ok(cid)
    }

    /// Hash data using IPFS
    pub async fn hash<R>(&self, data: R) -> Result<Cid, IpfsRpcError>
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
            .pin_ls(Some(&format!("{}", cid.to_string())), None)
            .await?;
        let keys = response.keys;
        // Check if the cid is pinned
        Ok(keys.contains_key(&cid.to_string()))
    }

    /// Get Block from IPFS
    pub async fn get_block(&self, cid: &Cid) -> Result<Vec<u8>, IpfsRpcError> {
        let stream = self.block_get(&cid.to_string());
        let block_data = stream.map_ok(|chunk| chunk.to_vec()).try_concat().await?;
        Ok(block_data)
    }
}

//
impl BlockStore for IpfsRpc {
    fn put_block_keyed(
        &self,
        cid: Cid,
        bytes: impl Into<Bytes> + wnfs::common::utils::CondSend,
    ) -> impl futures_util::Future<Output = Result<(), wnfs::common::BlockStoreError>>
           + wnfs::common::utils::CondSend {
        let bytes = bytes.into();
        let client = self.clone();

        async move {
            let response = tokio::task::spawn_blocking(move || {
                let cursor = std::io::Cursor::new(bytes.clone());
                tokio::runtime::Handle::current().block_on(client.add(cursor).map_err(|e| {
                    wnfs::common::BlockStoreError::Custom(
                        anyhow::anyhow!("ipfs error: could not put keyed block {e}").into(),
                    )
                }))
            })
            .await
            .map_err(|e| {
                wnfs::common::BlockStoreError::Custom(
                    anyhow::anyhow!("blockstore tokio runtime error: {e}").into(),
                )
            })??;

            if response != cid {
                return Err(wnfs::common::BlockStoreError::Custom(
                    anyhow::anyhow!("mismatched cid").into(),
                ));
            }

            Ok(())
        }
    }

    fn get_block(
        &self,
        cid: &Cid,
    ) -> impl futures_util::Future<Output = Result<Bytes, wnfs::common::BlockStoreError>>
           + wnfs::common::utils::CondSend {
        let cid = cid.clone();
        let client = self.clone();

        async move {
            let response = tokio::task::spawn_blocking(move || {
                tokio::runtime::Handle::current().block_on(client.get_block(&cid).map_err(|e| {
                    wnfs::common::BlockStoreError::Custom(
                        anyhow::anyhow!("ipfs error: could not get block {e}").into(),
                    )
                }))
            })
            .await
            .map_err(|e| {
                wnfs::common::BlockStoreError::Custom(
                    anyhow::anyhow!("blockstore tokio runtime error: {e}").into(),
                )
            })??;
            Ok(Bytes::from(response))
        }
    }

    fn has_block(
        &self,
        cid: &Cid,
    ) -> impl futures_util::Future<Output = Result<bool, wnfs::common::BlockStoreError>>
           + wnfs::common::utils::CondSend {
        let cid = cid.clone();
        let client = self.clone();

        async move {
            let response = tokio::task::spawn_blocking(move || {
                tokio::runtime::Handle::current().block_on(client.stat_cid(&cid).map_err(|e| {
                    wnfs::common::BlockStoreError::Custom(
                        anyhow::anyhow!("ipfs error: could not get block {e}").into(),
                    )
                }))
            })
            .await
            .map_err(|e| {
                wnfs::common::BlockStoreError::Custom(
                    anyhow::anyhow!("blockstore tokio runtime error: {e}").into(),
                )
            })??;
            Ok(response)
        }
    }
}

impl TryFrom<Url> for IpfsRpc {
    type Error = IpfsRpcError;
    fn try_from(url: Url) -> Result<Self, IpfsRpcError> {
        let scheme = Scheme::try_from(url.scheme())?;
        let username = url.username();
        let maybe_password = url.password();
        let host_str = url
            .host_str()
            .ok_or(IpfsRpcError::Url(url::ParseError::EmptyHost))?;
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
    #[error("Failed to build client: {0}")]
    Client(#[from] IpfsClientError),
    #[error("cid error")]
    Cid(#[from] wnfs::common::libipld::cid::Error),
}

mod tests {
    use super::*;
    use futures_util::task::waker;
    use std::convert::TryInto;
    use wnfs::common::libipld::Cid;
    use wnfs::common::BlockStore;
    #[tokio::test]
    async fn test_ipfs_rpc() {
        let url: Url = "http://localhost:5001".try_into().unwrap();
        let ipfs = IpfsRpc::try_from(url).unwrap();
        let data = "hello world".as_bytes();
        let cid = ipfs.hash(data).await.unwrap();
        let cid2 = ipfs.add(data).await.unwrap();
        assert_eq!(cid, cid2);
        let has_block = ipfs.has_block(&cid).await.unwrap();
        assert!(has_block);
        let block = ipfs.get_block(&cid).await.unwrap();
        assert_eq!(block, data);
    }

    #[tokio::test]
    async fn test_ipfs_rpc_block_store() {
        let url: Url = "http://localhost:5001".try_into().unwrap();
        let ipfs = IpfsRpc::try_from(url).unwrap();
        test_block_store(ipfs).await;
    }

    async fn test_block_store<T: BlockStore>(block_store: T) {
        let data = "hello world".as_bytes();

        // TODO: better on demand hashing solution
        let url: Url = "http://localhost:5001".try_into().unwrap();
        let ipfs = IpfsRpc::try_from(url).unwrap();
        let cid = ipfs.hash(data).await.unwrap();

        block_store
            .put_block_keyed(cid.clone(), data)
            .await
            .unwrap();
        let has_block = block_store.has_block(&cid).await.unwrap();
        assert!(has_block);
        let block = block_store.get_block(&cid).await.unwrap();
        assert_eq!(block, data);
    }
}
