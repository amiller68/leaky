use std::sync::Arc;
use std::sync::Mutex;

use libipld::block::Block;
use libipld::cbor::DagCborCodec;
use libipld::ipld::Ipld;
use libipld::store::DefaultParams;
use url::Url;

mod ipfs_rpc;

pub use ipfs_rpc::{Cid, IpfsRpc, IpfsRpcError, IpldCodec, MhCode};

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

    async fn get_object(&self, cid: &Cid) -> Result<Object, BackendError> {
        let data = self.ipfs_rpc.get_block(cid).await?;
        let block = Block::<DefaultParams>::new(cid.clone(), data).unwrap();
        let ipld = block.decode::<DagCborCodec, Ipld>().unwrap();
        let object = Object::from_ipld(&ipld).unwrap();
        Ok(object)
    }

    async fn put_object(&self, object: &Object) -> Result<Cid, BackendError> {
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
}

#[derive(Debug, thiserror::Error)]
pub enum BackendError {
    #[error("blockstore error: {0}")]
    IpfsRpc(#[from] IpfsRpcError),
    #[error("serde error: {0}")]
    Serde(#[from] serde_json::Error),
}

#[cfg(test)]
mod test {
    use super::*;

    #[tokio::test]
    async fn roundtrip_object() {
        let backend = Backend::default();
        let object = Object::default();
        let cid = backend.put_object(&object).await.unwrap();
        let object2 = backend.get_object(&cid).await.unwrap();
        assert_eq!(object, object2);
    }
}
