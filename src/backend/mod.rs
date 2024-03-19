use std::collections::HashMap;
use std::convert::TryFrom;
use std::io::Read;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;

use libipld::block::Block;
use libipld::cbor::DagCborCodec;
use libipld::ipld::Ipld;
use libipld::store::DefaultParams;
use url::Url;

mod ipfs_rpc;

use ipfs_rpc::{IpfsRpc, IpfsRpcError};

use crate::types::{Cid, IpldCodec, Manifest, MhCode, Node, Object};

#[derive(Clone)]
pub struct Backend {
    ipfs_rpc: IpfsRpc,
    manifest: Option<Arc<Mutex<Manifest>>>,
    block_cache: Arc<Mutex<HashMap<Cid, Ipld>>>,
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
            manifest: None,
            block_cache: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    pub async fn init(&mut self) -> Result<(), BackendError> {
        let node = Node::default();
        let cid = self.put_block_cache::<Node>(&node).await?;
        // Set the root cid in the manifest
        let mut manifest = Manifest::default();
        manifest.set_root(cid);
        self.manifest = Some(Arc::new(Mutex::new(manifest)));
        Ok(())
    }

    #[async_recursion::async_recursion]
    async fn pull_links(&mut self, cid: &Cid) -> Result<(), BackendError> {
        // Check if the manifest is already in the cache
        // If it is, return early
        if let Some(manifest) = self.manifest.as_ref() {
            return Ok(());
        }

        // Get the manifest from ipfs_rpc
        let manifest = self.get::<Manifest>(cid).await?;

        // Iterate over the objects and pull them from ipfs_rpc
        for (path, cid) in manifest.objects().iter() {
            // Chache the object in the block_cache
            let object = self.get::<Ipld>(cid).await?;
            self.block_cache
                .lock()
                .unwrap()
                .insert(cid.clone(), object.clone());
        }

        // Recurse from down the root node, pulling all the nodes
        // and caching them in the block_cache
        let root = manifest.root();
        self.pull_link(root).await?;

        // Cache the manifest
        self.manifest = Some(Arc::new(Mutex::new(manifest)));

        Ok(())
    }

    pub async fn push_links(&mut self) -> Result<Cid, BackendError> {
        let manifest = self.manifest.as_ref().unwrap().lock().unwrap();

        // Iterate over the block cache and push all the blocks to ipfs_rpc
        for (cid, object) in self.block_cache.lock().unwrap().iter() {
            self.put::<Ipld>(&object).await?;
        }

        // Iterate over the objects and push them to ipfs_rpc
        // and cache them in the block_cache
        let cid = self.put::<Manifest>(&manifest).await?;

        // Uhh that should be it
        Ok(cid)
    }

    #[async_recursion::async_recursion]
    pub async fn pull_link(&mut self, cid: &Cid) -> Result<(), BackendError> {
        let node = self.get::<Node>(cid).await?;
        self.block_cache
            .lock()
            .unwrap()
            .insert(cid.clone(), node.clone().into());
        // Recurse from down the root node, pulling all the nodes
        for (_name, link) in node.clone().iter() {
            match link {
                Ipld::Link(cid) => {
                    // Check if this is raw data
                    if cid.codec() == 0x55 {
                        return Ok(());
                    };
                    self.pull_link(cid).await?;
                }
                _ => panic!("not a link"),
            }
        }
        Ok(())
    }

    pub async fn add_object(self, path: PathBuf, object: &Object) -> Result<(), BackendError> {
        println!("add_object");
        let data = object.data();
        // Push the object to ipfs_rpc
        let cid = self.put::<Object>(&object).await.unwrap();

        println!("put object");
        // Cache the object in the block_cache
        // so we can use it later
        self.block_cache
            .lock()
            .unwrap()
            .insert(cid.clone(), object.clone().into());

        println!("insert path");
        // Link the object within it's path in the root node
        self.insert_path(path.clone(), &data).await?;

        println!("update manifest");
        // Add the object to the manifest
        let mut manifest = self.manifest.as_ref().unwrap().lock().unwrap();
        manifest.link_object(path.clone(), cid);

        Ok(())
    }

    async fn insert_path(&self, path: PathBuf, cid: &Cid) -> Result<(), BackendError> {
        println!("insert_path");
        let mut manifest = self.manifest.as_ref().unwrap().lock().unwrap();
        let root = manifest.root();

        println!("get root");
        // This is gaurenteed to specify a node
        let mut node = self.get::<Node>(&root).await?;

        // Upsert the path in the node
        let cid = self.upsert_path(&mut node, &path, cid).await?;

        // Update the root node
        manifest.set_root(cid);

        Ok(())
    }

    #[async_recursion::async_recursion]
    async fn upsert_path(
        &self,
        node: &mut Node,
        path: &PathBuf,
        cid: &Cid,
    ) -> Result<Cid, BackendError> {
        // Check if the path is empty
        // This should never happen -- it means we're trying to insert a file
        // into the place of a directory
        if path.iter().count() == 0 {
            panic!("path is empty");
        }

        // Check if this is the last part of the path
        // If it is, we're done so -- just update the node and return
        if path.iter().count() == 1 {
            let name = path.iter().next().unwrap().to_string_lossy().to_string();
            node.add(&name, cid);
            // Put the node into the block_cache
            let cid = self.put_block_cache::<Node>(&node).await?;
            return Ok(cid);
        }

        // Get the next part of the path
        let next = path.iter().next().unwrap().to_string_lossy().to_string();
        // Get the remaining path
        let remaining = path.iter().skip(1).collect::<PathBuf>();

        if let Some(next_cid) = node.get(&next) {
            let mut nn = self.get_block_cache::<Node>(&next_cid).await?;
            let cid = self.upsert_path(&mut nn, &remaining, cid).await?;
            node.add(&next, &cid);
            let cid = self.put_block_cache::<Node>(&node).await?;
            Ok(cid)
        } else {
            let mut nn = Node::default();
            let cid = self.create_path(&mut nn, &remaining, cid).await?;
            node.add(&next, &cid);
            let cid = self.put_block_cache::<Node>(&node).await?;
            Ok(cid)
        }
    }

    #[async_recursion::async_recursion]
    async fn create_path(
        &self,
        node: &mut Node,
        path: &PathBuf,
        cid: &Cid,
    ) -> Result<Cid, BackendError> {
        // Check if the path is empty
        // This should never happen as we don't create empty directories
        if path.iter().count() == 0 {
            panic!("path is empty");
        }

        // Check if this is the last part of the path
        // If it is, we're done so -- just update the node with the new link
        // Store the node in the block_cache
        // and return the cid
        if path.iter().count() == 1 {
            let name = path.iter().next().unwrap().to_string_lossy();
            node.add(&name, cid);
            // Put the node into the block_cache
            let cid = self.put_block_cache::<Node>(node).await?;
            return Ok(cid);
        }
        // Get the next part of the path
        let next = path.iter().next().unwrap().to_string_lossy().to_string();
        // Get the remaining path
        let remaining = path.iter().skip(1).collect::<PathBuf>();
        let mut next_node = if let Some(next_cid) = node.get(&next) {
            self.get_block_cache::<Node>(&next_cid).await?
        } else {
            Node::default()
        };
        let next_node_cid = self.create_path(&mut next_node, &remaining, cid).await?;
        node.add(&next, &next_node_cid);
        let cid = self.put_block_cache::<Node>(&node).await?;
        Ok(cid)
    }

    async fn add_data<R>(&self, data: R) -> Result<Cid, BackendError>
    where
        R: Read + Send + Sync + 'static + Unpin,
    {
        let cid = self.ipfs_rpc.add_data(MhCode::Blake3_256, data).await?;
        Ok(cid)
    }

    async fn put<B>(&self, object: &B) -> Result<Cid, BackendError>
    where
        B: Into<Ipld> + Clone,
    {
        let ipld: Ipld = object.clone().into();
        let block =
            Block::<DefaultParams>::encode(DagCborCodec, MhCode::Blake3_256, &ipld).unwrap();
        let cursor = std::io::Cursor::new(block.data().to_vec());
        let cid = self
            .ipfs_rpc
            .put_block(IpldCodec::DagCbor, MhCode::Blake3_256, cursor)
            .await?;
        Ok(cid)
    }

    async fn get_block_cache<B>(&self, cid: &Cid) -> Result<B, BackendError>
    where
        B: TryFrom<Ipld>,
    {
        let block_cache = self.block_cache.lock().unwrap();
        let ipld = block_cache.get(cid).unwrap();
        let object = B::try_from(ipld.clone()).map_err(|_| BackendError::Ipld)?;
        Ok(object)
    }

    async fn put_block_cache<B>(&self, object: &B) -> Result<Cid, BackendError>
    where
        B: Into<Ipld> + Clone,
    {
        let block = Block::<DefaultParams>::encode(
            DagCborCodec,
            MhCode::Blake3_256,
            &object.clone().into(),
        )
        .unwrap();
        let cid = block.cid().clone();

        self.block_cache
            .lock()
            .unwrap()
            .insert(cid.clone(), object.clone().into());
        Ok(cid.clone())
    }

    async fn get<B>(&self, cid: &Cid) -> Result<B, BackendError>
    where
        B: TryFrom<Ipld>,
    {
        let data = self.ipfs_rpc.get_block_send_safe(cid).await?;
        let block = Block::<DefaultParams>::new(cid.clone(), data).unwrap();
        let ipld = block.decode::<DagCborCodec, Ipld>().unwrap();
        let object = B::try_from(ipld).map_err(|_| BackendError::Ipld)?;
        Ok(object)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum BackendError {
    #[error("blockstore error: {0}")]
    IpfsRpc(#[from] IpfsRpcError),
    #[error("serde error: {0}")]
    Serde(#[from] serde_json::Error),
    #[error("could not convert Ipld to type")]
    Ipld,
}

#[cfg(test)]
mod test {
    use super::*;

    use crate::types::Object;

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
        let manifest = Manifest::default();
        let cid = backend.put::<Manifest>(&manifest).await.unwrap();
        let manifest2 = backend.get::<Manifest>(&cid).await.unwrap();
        assert_eq!(manifest, manifest2);
    }

    #[tokio::test]
    async fn roundtrip_node() {
        let backend = Backend::default();
        let node = Node::default();
        let cid = backend.put::<Node>(&node).await.unwrap();
        let node2 = backend.get::<Node>(&cid).await.unwrap();
        assert_eq!(node, node2);
    }

    #[tokio::test]
    async fn insert_object() {
        let mut backend = Backend::default();
        backend.init().await.unwrap();
        // Make a simple object around some raw data
        let mut object = Object::default();
        let data_cid = backend.add_data("foo".as_bytes()).await.unwrap();
        object.update(Some(data_cid), None);
        let path = PathBuf::from("foo/buzz/bar");
        backend
            .clone()
            .add_object(path.clone(), &object)
            .await
            .unwrap();
        let cid = backend.push_links().await.unwrap();

        let mut backend_2 = Backend::default();
        backend_2.pull_links(&cid).await.unwrap();

        assert_eq!(
            backend.manifest.unwrap().lock().unwrap().root(),
            backend_2.manifest.unwrap().lock().unwrap().root()
        );
    }
}
