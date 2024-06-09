use std::convert::TryFrom;
use std::io::Read;
use std::ops::Deref;
use std::str::FromStr;

use futures_util::TryStreamExt;
use http::uri::Scheme;
use ipfs_api_backend_hyper::request::{Add as AddRequest, BlockPut as BlockPutRequest};
use ipfs_api_backend_hyper::IpfsApi;
use ipfs_api_backend_hyper::TryFromUri;
use serde::{Deserialize, Serialize};
use url::Url;

use crate::types::{Cid, IpldCodec, MhCode};

pub use ipfs_api_backend_hyper::IpfsClient;

/* Constants */

const DEFAULT_CID_VERSION: u32 = 1;
const DEFAULT_MH_TYPE: &str = "blake3";

/* Ipfs Rpc Client Wrapper */

#[derive(Clone)]
pub struct IpfsRpc<T: Send>(T);

impl Default for IpfsRpc<IpfsClient> {
    fn default() -> Self {
        let url: Url = "http://localhost:5001".try_into().unwrap();
        Self::try_from(url).unwrap()
    }
}

// TODO: make this less convoluted
impl TryFrom<Url> for IpfsRpc<IpfsClient> {
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

impl Deref for IpfsRpc<IpfsClient> {
    type Target = IpfsClient;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl IpfsRpc<IpfsClient> {
    // TODO: LOCALIZE
    // Hash raw data using the specified hash function
    // # Arguments
    // * code: the multihash code to use for the hash
    // * data: the data to hash. This can be anything that implements Read. Should be safely passable between threads
    // # Returns
    // * the Cid of the data
    pub async fn hash_data<R>(&self, code: MhCode, data: R) -> Result<Cid, IpfsRpcError>
    where
        R: Read + Send + Sync + 'static + Unpin,
    {
        let hash = match code {
            MhCode::Blake3_256 => "blake3",
            MhCode::Sha3_256 => "sha3-256",
            _ => DEFAULT_MH_TYPE,
        };
        let mut options = AddRequest::default();
        options.hash = Some(hash);
        options.cid_version = Some(DEFAULT_CID_VERSION);
        options.only_hash = Some(true);
        let response = self.add_with_options(data, options).await?;
        let cid = Cid::from_str(&response.hash)?;
        Ok(cid)
    }

    /// Add raw data to Ipfs. This will implement chunking for you
    /// Do not use over data where you need control over codecs and chunking
    /// # Arguments
    /// * code: the multihash code to use for the block
    /// * data: the data to add. This can be anything that implements Read. Should be safely passable between threads
    /// # Returns
    /// * the Cid of the data
    // NOTE: this does not support ALL MhCodes. If an unsupported code is passed, it will use our
    // default of blake3
    pub async fn add_data<R>(&self, code: MhCode, data: R) -> Result<Cid, IpfsRpcError>
    where
        R: Read + Send + Sync + 'static + Unpin,
    {
        let hash = match code {
            MhCode::Blake3_256 => "blake3",
            MhCode::Sha3_256 => "sha3-256",
            _ => DEFAULT_MH_TYPE,
        };

        let mut options = AddRequest::default();
        options.hash = Some(hash);
        options.cid_version = Some(DEFAULT_CID_VERSION);

        let response = self.add_with_options(data, options).await?;
        let cid = Cid::from_str(&response.hash)?;

        Ok(cid)
    }

    /// Get raw data from Ipfs. This will traverse the dag and return the raw data
    /// # Arguments
    /// * cid: the Cid of the data to get
    /// # Returns
    /// * the raw data
    /// Note: this will not return the raw data if the data is not raw
    /// Note: this will not return the raw data if the data is not pinned
    pub async fn cat_data(&self, cid: &Cid) -> Result<Vec<u8>, IpfsRpcError> {
        let response_stream = self
            .cat(&cid.to_string())
            .map_ok(|chunk| chunk.to_vec())
            .try_concat()
            .await?;
        let response = response_stream;
        Ok(response)
    }

    /// Put a block to the RPC endpoint. Provides control over the codec and multihash
    /// # Arguments
    /// * codec: the codec to use for the block
    /// * code: the multihash code to use for the block
    /// * data: the data to add. This can be anything that implements Read. Should be safely passable between threads
    /// # Returns
    /// * the Cid of the data
    /// Note: this does not support ALL MhCodes. If an unsupported code is passed, it will use our
    /// default of blake3
    pub async fn put_block<R>(
        &self,
        codec: IpldCodec,
        code: MhCode,
        data: R,
    ) -> Result<Cid, IpfsRpcError>
    where
        R: Read + Send + Sync + 'static + Unpin,
    {
        // TODO: janky, i would like a more robust codec impl that aligns with the ipfs rpc api
        let cic_codec = match codec {
            IpldCodec::DagCbor => "dag-cbor",
            IpldCodec::DagJson => "dag-json",
            IpldCodec::DagPb => "dag-pb",
            IpldCodec::Raw => "raw",
        };

        // TODO: again, there's not just an easy way to match this to whatver the ipfs api expects
        //  For now let's just support blake3 and sha3-256
        let mhtype = match code {
            MhCode::Blake3_256 => "blake3",
            MhCode::Sha3_256 => "sha3-256",
            _ => DEFAULT_MH_TYPE,
        };

        let mut options = BlockPutRequest::default();
        options.mhtype = Some(mhtype);
        options.cid_codec = Some(cic_codec);
        options.pin = Some(true);

        let response = self.block_put_with_options(data, options).await?;

        let hash = response.key;

        let cid = Cid::from_str(&hash)?;

        Ok(cid)
    }

    /// Check if the RPC endpoint is pinning the specified CID
    pub async fn has_block(&self, cid: &Cid) -> Result<bool, IpfsRpcError> {
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

    pub async fn get_block_send_safe(&self, cid: &Cid) -> Result<Vec<u8>, IpfsRpcError> {
        let cid = *cid;
        let client = self.clone();
        let response = tokio::task::spawn_blocking(move || {
            tokio::runtime::Handle::current()
                .block_on(client.get_block(&cid))
                .map_err(IpfsRpcError::from)
        })
        .await
        .map_err(|e| {
            IpfsRpcError::Default(anyhow::anyhow!("blockstore tokio runtime error: {e}"))
        })??;

        Ok(response)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum IpfsRpcError {
    #[error("default error: {0}")]
    Default(#[from] anyhow::Error),
    #[error("url parse error")]
    Url(#[from] url::ParseError),
    #[error("http error")]
    Http(#[from] http::Error),
    #[error("Failed to parse scheme")]
    Scheme(#[from] http::uri::InvalidUri),
    #[error("Failed to build client: {0}")]
    Client(#[from] ipfs_api_backend_hyper::Error),
    #[error("cid error")]
    Cid(#[from] wnfs::common::libipld::cid::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Generate a random 1 KB reader
    fn random_reader() -> impl Read {
        use rand::Rng;
        use std::io::Cursor;
        let mut rng = rand::thread_rng();
        let data: Vec<u8> = (0..1024).map(|_| rng.gen()).collect();
        Cursor::new(data)
    }

    #[tokio::test]
    async fn test_add_data_sha3_256() {
        let ipfs = IpfsRpc::default();
        let data = random_reader();
        let mh_code = MhCode::Sha3_256;
        let cid = ipfs.add_data(mh_code, data).await.unwrap();
        assert_eq!(cid.version(), libipld::cid::Version::V1);
        assert_eq!(IpldCodec::try_from(cid.codec()).unwrap(), IpldCodec::Raw);
        assert_eq!(cid.hash().code(), 0x16);
    }

    #[tokio::test]
    async fn test_add_data_cat_data() {
        let ipfs = IpfsRpc::default();
        let data = std::io::Cursor::new(b"hello world");
        let mh_code = MhCode::Sha3_256;
        let cid = ipfs.add_data(mh_code, data).await.unwrap();
        let cat_data = ipfs.cat_data(&cid).await.unwrap();
        assert_eq!(cat_data.len(), 11);
        assert_eq!(cat_data, b"hello world");
    }

    #[tokio::test]
    async fn test_add_data_blake3_256() {
        let ipfs = IpfsRpc::default();
        let data = random_reader();
        let mh_code = MhCode::Blake3_256;
        let cid = ipfs.add_data(mh_code, data).await.unwrap();
        assert_eq!(cid.version(), libipld::cid::Version::V1);
        assert_eq!(IpldCodec::try_from(cid.codec()).unwrap(), IpldCodec::Raw);
        assert_eq!(cid.hash().code(), 0x1e);
    }

    #[tokio::test]
    async fn test_put_block_sha3_256_raw() {
        let ipfs = IpfsRpc::default();
        let data = random_reader();
        let mh_code = MhCode::Sha3_256;
        let codec = IpldCodec::Raw;
        let cid = ipfs.put_block(codec, mh_code, data).await.unwrap();
        assert_eq!(cid.version(), libipld::cid::Version::V1);
        assert_eq!(IpldCodec::try_from(cid.codec()).unwrap(), IpldCodec::Raw);
        assert_eq!(cid.hash().code(), 0x16);
    }

    #[tokio::test]
    async fn test_put_block_blake3_256_raw() {
        let ipfs = IpfsRpc::default();
        let data = random_reader();
        let mh_code = MhCode::Blake3_256;
        let codec = IpldCodec::Raw;
        let cid = ipfs.put_block(codec, mh_code, data).await.unwrap();
        assert_eq!(cid.version(), libipld::cid::Version::V1);
        assert_eq!(IpldCodec::try_from(cid.codec()).unwrap(), IpldCodec::Raw);
        assert_eq!(cid.hash().code(), 0x1e);
    }
    #[tokio::test]
    async fn test_put_block_sha3_256_dag_cbor() {
        let ipfs = IpfsRpc::default();
        let data = random_reader();
        let mh_code = MhCode::Sha3_256;
        let codec = IpldCodec::DagCbor;
        let cid = ipfs.put_block(codec, mh_code, data).await.unwrap();
        assert_eq!(cid.version(), libipld::cid::Version::V1);
        assert_eq!(
            IpldCodec::try_from(cid.codec()).unwrap(),
            IpldCodec::DagCbor
        );
        assert_eq!(cid.hash().code(), 0x16);
    }

    #[tokio::test]
    async fn test_put_block_blake3_256_dag_cbor() {
        let ipfs = IpfsRpc::default();
        let data = random_reader();
        let mh_code = MhCode::Blake3_256;
        let codec = IpldCodec::DagCbor;
        let cid = ipfs.put_block(codec, mh_code, data).await.unwrap();
        assert_eq!(cid.version(), libipld::cid::Version::V1);
        assert_eq!(
            IpldCodec::try_from(cid.codec()).unwrap(),
            IpldCodec::DagCbor
        );
        assert_eq!(cid.hash().code(), 0x1e);
    }
}
