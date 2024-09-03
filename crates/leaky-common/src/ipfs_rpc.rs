use std::convert::TryFrom;
use std::io::Read;
use std::path::PathBuf;
use std::str::FromStr;

use futures_util::TryStreamExt;
use http::uri::Scheme;
use ipfs_api_backend_hyper::request::{Add as AddRequest, BlockPut as BlockPutRequest};
use ipfs_api_backend_hyper::{IpfsApi, IpfsClient, TryFromUri};
use url::Url;

use crate::types::{Cid, IpldCodec, MhCode};

const DEFAULT_CID_VERSION: u32 = 1;
const DEFAULT_MH_TYPE: &str = "blake3";

#[derive(Clone)]
pub struct IpfsRpc {
    client: IpfsClient,
}

impl Default for IpfsRpc {
    fn default() -> Self {
        let url: Url = "http://localhost:5001".try_into().unwrap();
        Self::try_from(url).unwrap()
    }
}

impl TryFrom<Url> for IpfsRpc {
    type Error = IpfsRpcError;
    fn try_from(url: Url) -> Result<Self, IpfsRpcError> {
        let scheme = Scheme::try_from(url.scheme())?;
        let host_str = url
            .host_str()
            .ok_or(IpfsRpcError::Url(url::ParseError::EmptyHost))?;
        let port = url.port().unwrap_or(5001);
        let client = IpfsClient::from_host_and_port(scheme, host_str, port)?;
        Ok(Self { client })
    }
}

impl IpfsRpc {
    pub fn with_bearer_token(mut self, token: String) -> Self {
        self.client = self.client.with_bearer_token(token);
        self
    }

    pub fn with_path(mut self, path: &str) -> Self {
        let path = PathBuf::from(path);
        self.client = self.client.with_path(path);
        self
    }

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
        let client = self.client.clone();
        let response = tokio::task::spawn_blocking(move || {
            tokio::runtime::Handle::current()
                .block_on(async move { client.add_with_options(data, options).await })
        })
        .await
        .map_err(|e| IpfsRpcError::Default(anyhow::anyhow!("Join error: {}", e)))??;

        let cid = Cid::from_str(&response.hash)?;
        Ok(cid)
    }

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

        let client = self.client.clone();
        let response = tokio::task::spawn_blocking(move || {
            tokio::runtime::Handle::current()
                .block_on(async move { client.add_with_options(data, options).await })
        })
        .await
        .map_err(|e| IpfsRpcError::Default(anyhow::anyhow!("Join error: {}", e)))??;
        let cid = Cid::from_str(&response.hash)?;

        Ok(cid)
    }

    pub async fn cat_data(&self, cid: &Cid) -> Result<Vec<u8>, IpfsRpcError> {
        let client = self.client.clone();
        let cid_string = cid.to_string();

        // Spawn a blocking task to perform the potentially blocking operation
        let result = tokio::task::spawn_blocking(move || {
            tokio::runtime::Runtime::new()
                .unwrap()
                .block_on(async move {
                    client
                        .cat(&cid_string)
                        .map_ok(|chunk| chunk.to_vec())
                        .try_concat()
                        .await
                })
        })
        .await
        .map_err(|e| IpfsRpcError::Default(anyhow::anyhow!("Join error: {}", e)))??;

        Ok(result)
    }


    // NOTE: had to wrap the client call in a spawn_blocking because the client doesn't implement Send
    pub async fn put_block<R>(
        &self,
        codec: IpldCodec,
        code: MhCode,
        data: R,
    ) -> Result<Cid, IpfsRpcError>
    where
        R: Read + Send + Sync + 'static + Unpin,
    {
        let cic_codec = match codec {
            IpldCodec::DagCbor => "dag-cbor",
            IpldCodec::DagJson => "dag-json",
            IpldCodec::DagPb => "dag-pb",
            IpldCodec::Raw => "raw",
        };

        let mhtype = match code {
            MhCode::Blake3_256 => "blake3",
            MhCode::Sha3_256 => "sha3-256",
            _ => DEFAULT_MH_TYPE,
        };

        let mut options = BlockPutRequest::default();
        options.mhtype = Some(mhtype);
        options.cid_codec = Some(cic_codec);
        options.pin = Some(true);

        let client = self.client.clone();
        let result = tokio::task::spawn_blocking(move || {
            tokio::runtime::Handle::current()
                .block_on(async move { client.block_put_with_options(data, options).await })
        })
        .await
        .map_err(|e| IpfsRpcError::Default(anyhow::anyhow!("Join error: {}", e)))??;

        let cid = Cid::from_str(&result.key)?;

        Ok(cid)
    }

    pub async fn has_block(&self, cid: &Cid) -> Result<bool, IpfsRpcError> {
        let cid = *cid;
        let client = self.client.clone();
        let response = tokio::task::spawn_blocking(move || {
            tokio::runtime::Handle::current()
                .block_on(async move { client.pin_ls(Some(&cid.to_string()), None).await })
        })
        .await
        .map_err(|e| IpfsRpcError::Default(anyhow::anyhow!("Join error: {}", e)))??;

        let keys = response.keys;
        Ok(keys.contains_key(&cid.to_string()))
    }

    pub async fn get_block(&self, cid: &Cid) -> Result<Vec<u8>, IpfsRpcError> {
        let cid = *cid;
        let client = self.client.clone();
        tokio::task::spawn_blocking(move || {
            tokio::runtime::Handle::current().block_on(async move {
                let stream = client.block_get(&cid.to_string());
                let block_data = stream.map_ok(|chunk| chunk.to_vec()).try_concat().await?;
                Ok(block_data)
            })
        })
        .await
        .map_err(|e| IpfsRpcError::Default(anyhow::anyhow!("Join error: {}", e)))?
    }

    pub async fn get_block_send_safe(&self, cid: &Cid) -> Result<Vec<u8>, IpfsRpcError> {
        let cid = *cid;
        let client = self.client.clone();
        tokio::task::spawn_blocking(move || {
            tokio::runtime::Handle::current().block_on(async move {
                let stream = client.block_get(&cid.to_string());
                let block_data = stream.map_ok(|chunk| chunk.to_vec()).try_concat().await?;
                Ok(block_data)
            })
        })
        .await
        .map_err(|e| IpfsRpcError::Default(anyhow::anyhow!("Join error: {}", e)))?
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
    Cid(#[from] crate::types::CidError),
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
