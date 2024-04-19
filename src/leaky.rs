use std::collections::BTreeMap;
use std::collections::HashMap;
use std::convert::TryFrom;
use std::io::Read;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;

use libipld::block::Block;
use libipld::cbor::DagCborCodec;
use libipld::store::DefaultParams;
use url::Url;

use crate::ipfs_rpc::{IpfsRpc, IpfsRpcError};
use crate::types::{Cid, Ipld, IpldCodec, Manifest, MhCode, Node, Object};

#[derive(Clone)]
pub struct Leaky {
    ipfs_rpc: IpfsRpc,

    cid: Option<Cid>,
    manifest: Option<Arc<Mutex<Manifest>>>,
    block_cache: Arc<Mutex<HashMap<Cid, Ipld>>>,
}

impl Default for Leaky {
    fn default() -> Self {
        let ipfs_rpc_url = Url::parse("http://localhost:5001").unwrap();
        Self::new(ipfs_rpc_url).unwrap()
    }
}

impl Leaky {
    pub fn new(ipfs_rpc_url: Url) -> Result<Self, LeakyError> {
        let ipfs_rpc = IpfsRpc::try_from(ipfs_rpc_url)?;
        Ok(Self {
            ipfs_rpc,
            cid: None,
            manifest: None,
            block_cache: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    pub fn cid(&self) -> Result<Cid, LeakyError> {
        match self.cid {
            Some(cid) => Ok(cid),
            None => Err(LeakyError::NoCid),
        }
    }

    /* Sync functions */

    pub async fn init(&mut self) -> Result<(), LeakyError> {
        // Check if we have a cid
        if self.cid.is_some() {
            panic!("already initialized");
        }
        if self.manifest.is_some() {
            panic!("already initialized");
        }

        // Create a new root node
        let node = Node::default();
        // Put the node into the block_cache
        let cid = self.put_block_cache::<Node>(&node).await?;
        // Set the root cid in the manifest
        let mut manifest = Manifest::default();
        manifest.set_root(cid);

        let manifest_cid = self.put::<Manifest>(&manifest).await?;

        self.cid = Some(manifest_cid.clone());
        self.manifest = Some(Arc::new(Mutex::new(manifest)));
        Ok(())
    }

    pub async fn pull(&mut self, cid: &Cid) -> Result<(), LeakyError> {
        // Try to pull the manifest from our ipfs_rpc
        let manifest = self.get::<Manifest>(cid).await?;
        // Cool! now recurse on the root of the manifest
        // and pull all the links into our local cache

        self.pull_links(manifest.root()).await?;

        // Now just update the internal state and return
        self.cid = Some(cid.clone());
        self.manifest = Some(Arc::new(Mutex::new(manifest)));
        Ok(())
    }

    pub async fn push(&mut self) -> Result<(), LeakyError> {
        // Iterate over the block cache and push all the blocks to ipfs_rpc
        for (_cid, object) in self.block_cache.lock().unwrap().iter() {
            self.put::<Ipld>(&object).await?;
        }

        // Push the manifest to ipfs_rpc
        let manifest = self.manifest.as_ref().unwrap().lock().unwrap();
        let cid = self.put::<Manifest>(&manifest).await?;

        // Uhh that should be it
        self.cid = Some(cid.clone());
        Ok(())
    }

    /* Bucket functions */

    pub async fn add<R>(
        &mut self,
        path: PathBuf,
        data: R,
        maybe_metadata: Option<&BTreeMap<String, Ipld>>,
    ) -> Result<(), LeakyError>
    where
        R: Read + Send + Sync + 'static + Unpin,
    {
        let data_cid = self.add_data(data).await?;
        let mut manifest = self.manifest.as_ref().unwrap().lock().unwrap();
        let root_node_cid = manifest.root();
        let new_root_node_cid = self
            .upsert_link_and_object(&root_node_cid, &path, Some(&data_cid), maybe_metadata)
            .await?;
        manifest.set_root(new_root_node_cid);
        let manifest_cid = self.put::<Manifest>(&manifest).await?;
        self.cid = Some(manifest_cid);
        Ok(())
    }

    pub async fn ls(&self, path: PathBuf) -> Result<BTreeMap<String, Cid>, LeakyError> {
        let manifest = self.manifest.as_ref().unwrap().lock().unwrap();
        let root_node_cid = manifest.root();
        let node = self.get_block_cache::<Node>(&root_node_cid).await?;
        let mut node = node;

        // Iterate on the remaining path
        for part in path.iter() {
            let next = part.to_string_lossy().to_string();
            let next_cid = node.get_link(&next).unwrap();
            node = self.get_block_cache::<Node>(&next_cid).await?;
        }

        // Get the links from the node
        let links = node.get_links();
        Ok(links)
    }

    pub async fn cat(&self, path: PathBuf) -> Result<Vec<u8>, LeakyError> {
        let manifest = self.manifest.as_ref().unwrap().lock().unwrap();
        let root_node_cid = manifest.root();
        let node = self.get_block_cache::<Node>(&root_node_cid).await?;
        let mut node = node;
        // Get the dir path
        let dir_path = path
            .iter()
            .take(path.iter().count() - 1)
            .collect::<PathBuf>();
        // Get the file name
        let file_name = path.iter().last().unwrap().to_string_lossy().to_string();

        // Iterate on the remaining path
        for part in dir_path.iter() {
            let next = part.to_string_lossy().to_string();
            let next_cid = node.get_link(&next).unwrap();
            node = self.get_block_cache::<Node>(&next_cid).await?;
        }

        // Get the link from the node
        let link = node.get_link(&file_name).unwrap();
        let data = self.cat_data(&link).await?;

        Ok(data)
    }

    /* Helper functions */

    #[async_recursion::async_recursion]
    async fn pull_links(&mut self, cid: &Cid) -> Result<(), LeakyError> {
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
                    self.pull_links(cid).await?;
                }
                // Just ignore anything that's not a link
                _ => {}
            }
        }
        Ok(())
    }

    #[async_recursion::async_recursion]
    async fn upsert_link_and_object(
        &self,
        cid: &Cid,
        path: &PathBuf,
        maybe_link: Option<&Cid>,
        maybe_metadata: Option<&BTreeMap<String, Ipld>>,
    ) -> Result<Cid, LeakyError> {
        println!("upsert_link_and_object: {:?}", path);
        // Get the node we're going to update
        let mut node = self.get::<Node>(cid).await?;
        let next = path.iter().next().unwrap().to_string_lossy().to_string();

        // Determine if the path is empty
        if path.iter().count() == 0 {
            panic!("path is empty");
        }

        // Determine if this is the last part of the path
        match path.iter().count() {
            // Base case, just insert the link and object
            1 => {
                // Determine if we're inserting, updating, or deleting
                match maybe_link {
                    // Ok so we have a link, so we are either adding or updating
                    Some(link) => {
                        node.put_object_link(&next, link, maybe_metadata);
                    }
                    // Delete the link and object
                    None => {
                        node.del(&next);
                    }
                }
                let cid = self.put_block_cache::<Node>(&node).await?;
                Ok(cid)
            }
            // We gave more to recurse on
            _ => {
                // Get the next part of the path
                let remaining = path.iter().skip(1).collect::<PathBuf>();
                // Determine if the next part of the path exists within the tree
                let next_cid = if let Some(next_cid) = node.get_link(&next) {
                    next_cid.clone()
                } else {
                    // Ok create a new node to hold this part of the path
                    let mut new_node = Node::default();
                    self.put_block_cache::<Node>(&new_node).await?
                };
                // Upsert the remaining path components into the node
                let cid = &self
                    .upsert_link_and_object(&next_cid, &remaining, maybe_link, maybe_metadata)
                    .await?;
                // Insert the updated link
                node.put_link(&next, &cid);
                // Put the updated node
                let cid = self.put_block_cache::<Node>(&node).await?;
                // Okie doke!
                Ok(cid)
            }
        }
    }

    /* Data operations */

    async fn add_data<R>(&self, data: R) -> Result<Cid, LeakyError>
    where
        R: Read + Send + Sync + 'static + Unpin,
    {
        let cid = self.ipfs_rpc.add_data(MhCode::Blake3_256, data).await?;
        Ok(cid)
    }

    async fn cat_data(&self, cid: &Cid) -> Result<Vec<u8>, LeakyError> {
        let data = self.ipfs_rpc.cat_data(cid).await?;
        Ok(data)
    }

    async fn get<B>(&self, cid: &Cid) -> Result<B, LeakyError>
    where
        B: TryFrom<Ipld>,
    {
        let data = self.ipfs_rpc.get_block_send_safe(cid).await?;
        let block = Block::<DefaultParams>::new(cid.clone(), data).unwrap();
        let ipld = block.decode::<DagCborCodec, Ipld>().unwrap();
        let object = B::try_from(ipld).map_err(|_| LeakyError::Ipld)?;
        Ok(object)
    }

    async fn put<B>(&self, object: &B) -> Result<Cid, LeakyError>
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

    async fn get_block_cache<B>(&self, cid: &Cid) -> Result<B, LeakyError>
    where
        B: TryFrom<Ipld>,
    {
        let block_cache = self.block_cache.lock().unwrap();
        let ipld = block_cache.get(cid).unwrap();
        let object = B::try_from(ipld.clone()).map_err(|_| LeakyError::Ipld)?;
        Ok(object)
    }

    async fn put_block_cache<B>(&self, object: &B) -> Result<Cid, LeakyError>
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
}

#[derive(Debug, thiserror::Error)]
pub enum LeakyError {
    #[error("blockstore error: {0}")]
    IpfsRpc(#[from] IpfsRpcError),
    #[error("serde error: {0}")]
    Serde(#[from] serde_json::Error),
    #[error("could not convert Ipld to type")]
    Ipld,
    #[error("cid is not set")]
    NoCid,
}

#[cfg(test)]
mod test {
    use super::*;

    async fn empty_leaky_cid() -> Cid {
        let mut leaky = Leaky::default();
        leaky.init().await.unwrap();
        leaky.push().await.unwrap();
        leaky.cid().unwrap()
    }

    #[tokio::test]
    async fn pull_empty() {
        let cid = empty_leaky_cid().await;
        let mut leaky = Leaky::default();
        leaky.pull(&cid).await.unwrap();
        assert_eq!(leaky.cid().unwrap(), cid);
    }
    #[tokio::test]
    async fn add() {
        let cid = empty_leaky_cid().await;
        let mut leaky = Leaky::default();
        leaky.pull(&cid).await.unwrap();
        let data = "foo".as_bytes();
        leaky.add(PathBuf::from("foo"), data, None).await.unwrap();
    }

    #[tokio::test]
    async fn add_with_metadata() {
        let cid = empty_leaky_cid().await;
        let mut leaky = Leaky::default();
        leaky.pull(&cid).await.unwrap();
        let data = "foo".as_bytes();
        let mut metadata = BTreeMap::new();
        metadata.insert("foo".to_string(), Ipld::String("bar".to_string()));
        leaky
            .add(PathBuf::from("foo"), data, Some(&metadata))
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn add_cat() {
        let cid = empty_leaky_cid().await;
        let mut leaky = Leaky::default();
        leaky.pull(&cid).await.unwrap();
        let data = "foo".as_bytes();
        leaky.add(PathBuf::from("bar"), data, None).await.unwrap();
        let get_data = leaky.cat(PathBuf::from("bar")).await.unwrap();
        assert_eq!(data, get_data);
    }

    #[tokio::test]
    async fn add_ls() {
        let cid = empty_leaky_cid().await;
        let mut leaky = Leaky::default();
        leaky.pull(&cid).await.unwrap();
        let data = "foo".as_bytes();
        leaky.add(PathBuf::from("bar"), data, None).await.unwrap();
        let links = leaky.ls(PathBuf::from("")).await.unwrap();
        assert_eq!(links.len(), 1);
    }

    #[tokio::test]
    async fn add_deep() {
        let cid = empty_leaky_cid().await;
        let mut leaky = Leaky::default();
        leaky.pull(&cid).await.unwrap();
        let data = "foo".as_bytes();
        leaky
            .add(PathBuf::from("foo/bar/buzz"), data, None)
            .await
            .unwrap();
    }

    /*
        #[tokio::test]
        async fn roundtrip_object() {
            let backend = Leaky::default();
            let object = Object::default();
            let cid = backend.put::<Object>(&object).await.unwrap();
            let object2 = backend.get::<Object>(&cid).await.unwrap();
            assert_eq!(object, object2);
        }

        #[tokio::test]
        async fn roundtrip_manifest() {
            let backend = Leaky::default();
            let manifest = Manifest::default();
            let cid = backend.put::<Manifest>(&manifest).await.unwrap();
            let manifest2 = backend.get::<Manifest>(&cid).await.unwrap();
            assert_eq!(manifest, manifest2);
        }

        #[tokio::test]
        async fn roundtrip_node() {
            let backend = Leaky::default();
            let node = Node::default();
            let cid = backend.put::<Node>(&node).await.unwrap();
            let node2 = backend.get::<Node>(&cid).await.unwrap();
            assert_eq!(node, node2);
        }

        #[tokio::test]
        async fn insert_object() {
            let mut backend = Leaky::default();
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

            let mut backend_2 = Leaky::default();
            backend_2.pull_links(&cid).await.unwrap();

            assert_eq!(
                backend.manifest.unwrap().lock().unwrap().root(),
                backend_2.manifest.unwrap().lock().unwrap().root()
            );
        }
    */
}
