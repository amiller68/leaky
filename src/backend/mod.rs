use std::sync::Arc;
use std::sync::Mutex;

use libipld::block::Block;
use libipld::cbor::DagCborCodec;
use libipld::ipld::Ipld;
use libipld::store::DefaultParams;
use url::Url;

mod ipfs_rpc;

use ipfs_rpc::{IpfsRpc, IpfsRpcError};

use crate::types::{Cid, IpldCodec, MhCode};

use crate::traits::Blockable;
use crate::types::{Manifest, Object};

#[derive(Clone)]
pub struct Backend {
    ipfs_rpc: IpfsRpc,
    manifest: Arc<Mutex<Manifest>>,
}

impl Default for Backend {
    fn default() -> Self {
        let ipfs_rpc_url = Url::parse("http://localhost:5001").unwrap();
        Self::new(ipfs_rpc_url).unwrap()
    }
}

impl Backend {
    pub fn new(ipfs_rpc_url: Url) -> Result<Self, BackendError> {
        let ipfs_rpc = IpfsRpc::try_from(ipfs_rpc_url)?;
        Ok(Self {
            ipfs_rpc,
            manifest: Arc::new(Mutex::new(Manifest::new())),
        })
    }

    async fn put<B>(&self, object: &B) -> Result<Cid, BackendError>
    where
        B: Blockable,
    {
        let ipld = object.to_ipld();
        let block =
            Block::<DefaultParams>::encode(DagCborCodec, MhCode::Blake3_256, &ipld).unwrap();
        let cursor = std::io::Cursor::new(block.data().to_vec());
        let cid = self
            .ipfs_rpc
            .put_block(IpldCodec::DagCbor, MhCode::Blake3_256, cursor)
            .await?;
        Ok(cid)
    }

    async fn get<B>(&self, cid: &Cid) -> Result<B, BackendError>
    where
        B: Blockable,
    {
        let data = self.ipfs_rpc.get_block(cid).await?;
        let block = Block::<DefaultParams>::new(cid.clone(), data).unwrap();
        let ipld = block.decode::<DagCborCodec, Ipld>().unwrap();
        let object = B::from_ipld(&ipld).map_err(|_| BackendError::Blockable)?;
        Ok(object)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum BackendError {
    #[error("blockstore error: {0}")]
    IpfsRpc(#[from] IpfsRpcError),
    #[error("serde error: {0}")]
    Serde(#[from] serde_json::Error),
    #[error("blockable error")]
    Blockable,
}

#[cfg(test)]
mod test {
    use super::*;

    #[tokio::test]
    async fn roundtrip_object() {
        let backend = Backend::default();
        let object = Object::default();
        let cid = backend.put::<Object>(&object).await.unwrap();
        let object2 = backend.get::<Object>(&cid).await.unwrap();
        assert_eq!(object, object2);
    }

    #[tokio::test]
    async fn roundtrip_manifest() {
        let backend = Backend::default();
        let manifest = Manifest::new();
        let cid = backend.put::<Manifest>(&manifest).await.unwrap();
        let manifest2 = backend.get::<Manifest>(&cid).await.unwrap();
        assert_eq!(manifest, manifest2);
    }
}
