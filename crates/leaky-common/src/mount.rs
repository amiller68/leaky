use std::collections::BTreeMap;
use std::collections::HashMap;
use std::convert::TryFrom;
use std::io::Read;
use std::ops::{Deref, DerefMut};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::ipfs_rpc::{IpfsRpc, IpfsRpcError};
use crate::types::{
    Block, Cid, DagCborCodec, DefaultParams, Ipld, IpldCodec, Manifest, MhCode, Node, Object,
};

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct BlockCache(pub HashMap<String, Ipld>);

impl Deref for BlockCache {
    type Target = HashMap<String, Ipld>;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for BlockCache {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

// TODO: this should do more
pub fn clean_path(path: &PathBuf) -> PathBuf {
    // Check if the path is absolute
    if !path.is_absolute() {
        panic!("path is not absolute");
    }

    return path
        .iter()
        .skip(1)
        .map(|part| part.to_string_lossy().to_string())
        .collect::<PathBuf>();
}

#[derive(Clone)]
pub struct Mount {
    cid: Cid,
    manifest: Arc<Mutex<Manifest>>,
    block_cache: Arc<Mutex<BlockCache>>,
    ipfs_rpc: IpfsRpc,
}

impl Mount {
    pub fn cid(&self) -> &Cid {
        &self.cid
    }

    pub fn manifest(&self) -> Manifest {
        self.manifest.lock().unwrap().clone()
    }

    pub fn block_cache(&self) -> BlockCache {
        self.block_cache.lock().unwrap().clone()
    }

    /* Sync functions */

    pub async fn init(ipfs_rpc: &IpfsRpc) -> Result<Self, MountError> {
        let mut manifest = Manifest::default();
        let block_cache = Arc::new(Mutex::new(BlockCache::default()));
        let node = Node::default();
        let data_cid = Mount::put_cache::<Node>(&node, &block_cache).await?;
        manifest.set_data(data_cid);
        let cid = Mount::put::<Manifest>(&manifest, ipfs_rpc).await?;

        Ok(Self {
            cid,
            manifest: Arc::new(Mutex::new(manifest)),
            block_cache,
            ipfs_rpc: ipfs_rpc.clone(),
        })
    }

    pub async fn pull(cid: Cid, ipfs_rpc: &IpfsRpc) -> Result<Self, MountError> {
        let manifest = Mount::get::<Manifest>(&cid, ipfs_rpc).await?;
        let block_cache = Arc::new(Mutex::new(BlockCache::default()));

        Mount::pull_links(manifest.data(), &block_cache, Some(ipfs_rpc)).await?;

        Ok(Self {
            cid,
            manifest: Arc::new(Mutex::new(manifest)),
            block_cache,
            ipfs_rpc: ipfs_rpc.clone(),
        })
    }

    pub async fn push(&self) -> Result<(), MountError> {
        let ipfs_rpc = &self.ipfs_rpc;

        // Clone the block cache data to avoid holding the lock across await points
        let block_cache_data = {
            let block_cache = self.block_cache.lock().unwrap();
            block_cache.clone()
        };

        for (cid_str, ipld) in block_cache_data.iter() {
            let cid = Mount::put::<Ipld>(ipld, ipfs_rpc).await?;
            assert_eq!(cid.to_string(), cid_str.to_string());
        }

        Ok(())
    }

    /* Api */

    pub async fn add<R>(
        &mut self,
        path: &PathBuf,
        data: R,
        maybe_metadata: Option<&BTreeMap<String, Ipld>>,
        hash_only: bool,
    ) -> Result<Cid, MountError>
    where
        R: Read + Send + Sync + 'static + Unpin,
    {
        let ipfs_rpc = &self.ipfs_rpc;
        let block_cache = &self.block_cache;
        let path = clean_path(path);

        let data_cid;
        if hash_only {
            data_cid = Mount::hash_data(data, ipfs_rpc).await?;
        } else {
            data_cid = Mount::add_data(data, ipfs_rpc).await?;
        };
        let mut manifest = self.manifest.lock().unwrap();
        let data_node_cid = manifest.data();
        let maybe_new_data_node_cid = Mount::upsert_link_and_object(
            data_node_cid,
            &path,
            Some(&data_cid),
            maybe_metadata,
            ipfs_rpc,
            block_cache,
        )
        .await?;
        let new_data_node_cid = match maybe_new_data_node_cid {
            Some(cid) => cid,
            // No Change
            None => return Ok(data_cid),
        };
        manifest.set_data(new_data_node_cid);
        let manifest_cid = Mount::put::<Manifest>(&manifest, ipfs_rpc).await?;
        self.cid = manifest_cid;
        Ok(data_cid)
    }

    pub async fn tag(
        &mut self,
        path: &PathBuf,
        metadata: &BTreeMap<String, Ipld>,
    ) -> Result<(), MountError> {
        let ipfs_rpc = &self.ipfs_rpc;
        let block_cache = &self.block_cache;
        let path = clean_path(path);
        let mut manifest = self.manifest.lock().unwrap();

        let data_node_cid = manifest.data();
        let maybe_new_data_node_cid = Mount::upsert_link_and_object(
            data_node_cid,
            &path,
            None,
            Some(metadata),
            ipfs_rpc,
            block_cache,
        )
        .await?;
        let new_data_node_cid = match maybe_new_data_node_cid {
            Some(cid) => cid,
            // No Change
            None => return Ok(()),
        };
        manifest.set_data(new_data_node_cid);
        let manifest_cid = Mount::put::<Manifest>(&manifest, ipfs_rpc).await?;
        self.cid = manifest_cid;
        Ok(())
    }

    pub async fn rm(&mut self, path: &PathBuf) -> Result<(), MountError> {
        let ipfs_rpc = &self.ipfs_rpc;
        let block_cache = &self.block_cache;
        let path = clean_path(path);
        let mut manifest = self.manifest.lock().unwrap();
        let data_node_cid = manifest.data();
        let maybe_new_data_node_cid =
            Mount::upsert_link_and_object(data_node_cid, &path, None, None, ipfs_rpc, block_cache)
                .await?;
        let new_data_node_cid = match maybe_new_data_node_cid {
            Some(cid) => {
                // The root node was deleted
                if cid == Cid::default() {
                    let data_node = Node::default();
                    Mount::put_cache::<Node>(&data_node, block_cache).await?
                } else {
                    cid
                }
            }
            // No Change
            None => return Ok(()),
        };
        manifest.set_data(new_data_node_cid);
        let manifest_cid = Mount::put::<Manifest>(&manifest, ipfs_rpc).await?;
        self.cid = manifest_cid;
        Ok(())
    }

    pub async fn ls(
        &self,
        path: &PathBuf,
    ) -> Result<Vec<(String, (Cid, Option<Object>))>, MountError> {
        let block_cache = &self.block_cache;
        let path = clean_path(path);
        let data_node_cid = {
            let manifest = self.manifest.lock().unwrap();
            let mc = manifest.clone();
            *mc.data()
        };
        let mut node = Mount::get_cache::<Node>(&data_node_cid, block_cache).await?;

        // Iterate on the remaining path
        for part in path.iter() {
            let next = part.to_string_lossy().to_string();
            let next_cid = node.get_link(&next).unwrap();
            node = match Mount::get_cache::<Node>(&next_cid, block_cache).await {
                Ok(node) => node,
                Err(_) => {
                    return Err(MountError::PathNotDir(path));
                }
            }
        }

        // Get the links from the node
        let links: Vec<_> = node
            .get_links()
            .iter()
            .map(|(name, link)| {
                let object = node.get_object(name);
                (name.clone(), (*link, object))
            })
            .collect();

        Ok(links)
    }

    /// Return all the items in the bucket in order by path name
    pub async fn items(&self) -> Result<Vec<(PathBuf, Cid)>, MountError> {
        let root_items = self.recursive_items(&PathBuf::from("/")).await?;
        let mut sorted_items = root_items;
        sorted_items.sort_by(|a, b| a.0.cmp(&b.0));
        Ok(sorted_items)
    }

    pub async fn cat(&self, path: &PathBuf) -> Result<Vec<u8>, MountError> {
        let ipfs_rpc = &self.ipfs_rpc;
        let block_cache = &self.block_cache;

        let path = clean_path(path);
        let data_node_cid = {
            let manifest = self.manifest.lock().unwrap();
            let mc = manifest.clone();
            *mc.data()
        };
        let node = Mount::get_cache::<Node>(&data_node_cid, block_cache).await?;
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
            node = Mount::get_cache::<Node>(&next_cid, block_cache).await?;
        }

        // Get the link from the node
        let link = node.get_link(&file_name).unwrap();
        let data = Mount::cat_data(&link, ipfs_rpc).await?;

        Ok(data)
    }

    /// Recursively bubble up all the items from a path
    ///  in sorted order
    #[async_recursion::async_recursion]
    async fn recursive_items(&self, path: &PathBuf) -> Result<Vec<(PathBuf, Cid)>, MountError> {
        let mut items = vec![];
        let links = match self.ls(path).await {
            Ok(l) => l,
            Err(err) => match err {
                MountError::PathNotDir(_) => {
                    return Ok(items);
                }
                _ => return Err(err),
            },
        };
        for (name, (_link, object)) in links {
            // If this is a directory, recurse
            if object.is_none() {
                let mut path = path.clone();
                path.push(name);
                let mut next_items = self.recursive_items(&path).await?;
                items.append(&mut next_items);
            } else {
                let mut path = path.clone();
                path.push(name);
                items.push((path, _link));
            }
        }
        Ok(items)
    }

    #[async_recursion::async_recursion]
    async fn pull_links(
        cid: &Cid,
        block_cache: &Arc<Mutex<BlockCache>>,
        ipfs_rpc: Option<&IpfsRpc>,
    ) -> Result<(), MountError> {
        let node = if let Some(ipfs_rpc) = ipfs_rpc {
            Mount::get::<Node>(cid, ipfs_rpc).await?
        } else {
            Mount::get_cache::<Node>(cid, block_cache).await?
        };
        block_cache
            .lock()
            .unwrap()
            .insert(cid.to_string(), node.clone().into());

        // Recurse from down the data node, pulling all the nodes
        for (_name, link) in node.clone().iter() {
            match link {
                Ipld::Link(cid) => {
                    // Check if this is raw data
                    if cid.codec() == 0x55 {
                        return Ok(());
                    };
                    Mount::pull_links(cid, block_cache, ipfs_rpc).await?;
                }
                // Just ignore anything that's not a link
                _ => {}
            }
        }
        Ok(())
    }

    // TODO: this doesn't percolate deleted directories back up
    #[async_recursion::async_recursion]
    async fn upsert_link_and_object(
        cid: &Cid,
        path: &Path,
        maybe_link: Option<&Cid>,
        maybe_metadata: Option<&BTreeMap<String, Ipld>>,
        ipfs_rpc: &IpfsRpc,
        block_cache: &Arc<Mutex<BlockCache>>,
    ) -> Result<Option<Cid>, MountError> {
        let is_rm = maybe_link.is_none() && maybe_metadata.is_none();
        // Get the node we're going to update
        let mut node = Mount::get_cache::<Node>(cid, block_cache).await?;
        let next = path.iter().next().unwrap().to_string_lossy().to_string();

        // Determine if the path is empty
        if path.iter().count() == 0 {
            panic!("path is empty");
        }

        // Determine if this is the last part of the path
        match path.iter().count() {
            // Base case, just insert the link and object
            1 => {
                // Delete the link
                if is_rm {
                    let (maybe_link, _maybe_obj) = node.del(&next);

                    // There is no link to delete
                    if maybe_link.is_none() {
                        return Ok(None);
                    }

                    // Otherwise if there are no more links, delete the node
                    if node.size() == 0 {
                        return Ok(Some(Cid::default()));
                    }
                } else {
                    node.update_link(&next, maybe_link, maybe_metadata);
                }

                // The node is updated, put it back into the cache and return the new cid
                let cid = Mount::put_cache::<Node>(&node, block_cache).await?;
                Ok(Some(cid))
            }
            // We gave more to recurse on
            _ => {
                // Get the next part of the path
                let remaining = path.iter().skip(1).collect::<PathBuf>();
                // Determine if the next part of the path exists within the tree
                let next_cid = if let Some(next_cid) = node.get_link(&next) {
                    next_cid
                } else if !is_rm {
                    // Ok create a new node to hold this part of the path
                    let new_node = Node::default();
                    Mount::put_cache::<Node>(&new_node, block_cache).await?
                } else {
                    return Ok(None);
                };
                // Upsert the remaining path components into the node
                let maybe_cid = Mount::upsert_link_and_object(
                    &next_cid,
                    &remaining,
                    maybe_link,
                    maybe_metadata,
                    ipfs_rpc,
                    block_cache,
                )
                .await?;
                let cid = match maybe_cid {
                    Some(cid) => cid,
                    // No change, return the original cid
                    None => return Ok(None),
                };

                if cid == Cid::default() {
                    node.del(&next);
                    if node.size() == 0 {
                        return Ok(Some(Cid::default()));
                    }
                } else {
                    node.put_link(&next, &cid);
                }
                let cid = Mount::put_cache::<Node>(&node, block_cache).await?;
                Ok(Some(cid))
            }
        }
    }

    pub async fn hash_data<R>(data: R, ipfs_rpc: &IpfsRpc) -> Result<Cid, MountError>
    where
        R: Read + Send + Sync + 'static + Unpin,
    {
        let cid = ipfs_rpc.hash_data(MhCode::Blake3_256, data).await?;
        Ok(cid)
    }

    pub async fn add_data<R>(data: R, ipfs_rpc: &IpfsRpc) -> Result<Cid, MountError>
    where
        R: Read + Send + Sync + 'static + Unpin,
    {
        let cid = ipfs_rpc.add_data(MhCode::Blake3_256, data).await?;
        Ok(cid)
    }

    async fn cat_data(cid: &Cid, ipfs_rpc: &IpfsRpc) -> Result<Vec<u8>, MountError> {
        let data = ipfs_rpc.cat_data(cid).await?;
        Ok(data)
    }

    async fn get<B>(cid: &Cid, ipfs_rpc: &IpfsRpc) -> Result<B, MountError>
    where
        B: TryFrom<Ipld>,
    {
        let data = ipfs_rpc.get_block_send_safe(cid).await?;
        let block = Block::<DefaultParams>::new(*cid, data).unwrap();
        let ipld = block.decode::<DagCborCodec, Ipld>().unwrap();
        let object = B::try_from(ipld).map_err(|_| MountError::Ipld)?;
        Ok(object)
    }

    async fn put<B>(object: &B, ipfs_rpc: &IpfsRpc) -> Result<Cid, MountError>
    where
        B: Into<Ipld> + Clone,
    {
        let ipld: Ipld = object.clone().into();
        let block =
            Block::<DefaultParams>::encode(DagCborCodec, MhCode::Blake3_256, &ipld).unwrap();
        let cursor = std::io::Cursor::new(block.data().to_vec());
        let cid = ipfs_rpc
            .put_block(IpldCodec::DagCbor, MhCode::Blake3_256, cursor)
            .await?;
        Ok(cid)
    }

    async fn get_cache<B>(cid: &Cid, block_cache: &Arc<Mutex<BlockCache>>) -> Result<B, MountError>
    where
        B: TryFrom<Ipld> + Send,
    {
        let block_cache = block_cache.lock().unwrap();
        let cid_str = cid.to_string();
        let ipld = match block_cache.get(&cid_str) {
            Some(i) => i,
            None => return Err(MountError::BlockCacheMiss(*cid)),
        };
        let object = B::try_from(ipld.clone()).map_err(|_| MountError::Ipld)?;

        Ok(object)
    }

    async fn put_cache<B>(
        object: &B,
        block_cache: &Arc<Mutex<BlockCache>>,
    ) -> Result<Cid, MountError>
    where
        B: Into<Ipld> + Clone,
    {
        let block = Block::<DefaultParams>::encode(
            DagCborCodec,
            MhCode::Blake3_256,
            &object.clone().into(),
        )
        .unwrap();
        let cid = block.cid();

        block_cache
            .lock()
            .unwrap()
            .insert(cid.to_string(), object.clone().into());
        Ok(*cid)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum MountError {
    #[error("block cache miss: {0}")]
    BlockCacheMiss(Cid),
    #[error("blockstore error: {0}")]
    IpfsRpc(#[from] IpfsRpcError),
    #[error("serde error: {0}")]
    Serde(#[from] serde_json::Error),
    #[error("could not convert Ipld to type")]
    Ipld,
    #[error("cid is not set")]
    NoCid,
    #[error("path is not directory: {0}")]
    PathNotDir(PathBuf),
    #[error("path is not file: {0}")]
    PathNotFile(PathBuf),
}

#[cfg(test)]
mod test {
    use super::*;

    async fn empty_mount() -> Mount {
        let ipfs_rpc = IpfsRpc::default();
        let mount = Mount::init(&ipfs_rpc).await.unwrap();
        mount
    }

    #[tokio::test]
    async fn add() {
        let mut mount = empty_mount().await;
        let data = "foo".as_bytes();
        mount
            .add(&PathBuf::from("/foo"), data, None, true)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn add_with_metadata() {
        let mut mount = empty_mount().await;
        let data = "foo".as_bytes();
        let mut metadata = BTreeMap::new();
        metadata.insert("foo".to_string(), Ipld::String("bar".to_string()));
        mount
            .add(&PathBuf::from("/foo"), data, Some(&metadata), true)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn add_cat() {
        let mut mount = empty_mount().await;
        let data = "foo".as_bytes();
        mount
            .add(&PathBuf::from("/bar"), data, None, false)
            .await
            .unwrap();
        let get_data = mount.cat(&PathBuf::from("/bar")).await.unwrap();
        assert_eq!(data, get_data);
    }

    #[tokio::test]
    async fn add_ls() {
        let mut mount = empty_mount().await;
        let data = "foo".as_bytes();
        mount
            .add(&PathBuf::from("/bar"), data, None, true)
            .await
            .unwrap();
        let links = mount.ls(&PathBuf::from("/")).await.unwrap();
        assert_eq!(links.len(), 1);
    }

    #[tokio::test]
    async fn add_deep() {
        let mut mount = empty_mount().await;
        let data = "foo".as_bytes();
        mount
            .add(&PathBuf::from("/foo/bar/buzz"), data, None, true)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn add_rm() {
        let mut mount = empty_mount().await;
        let data = "foo".as_bytes();
        mount
            .add(&PathBuf::from("/foo/bar"), data, None, true)
            .await
            .unwrap();
        mount.rm(&PathBuf::from("/foo/bar")).await.unwrap();
    }

    #[tokio::test]
    async fn add_pull_ls() {
        let mut mount = empty_mount().await;
        let data = "foo".as_bytes();
        mount
            .add(&PathBuf::from("/bar"), data, None, true)
            .await
            .unwrap();
        let cid = mount.cid();
        mount.push().await.unwrap();

        let mount = Mount::pull(cid.clone(), &IpfsRpc::default()).await.unwrap();
        println!("about to pull");
        assert_eq!(mount.ls(&PathBuf::from("/")).await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn add_add_deep() {
        let mut mount = empty_mount().await;

        let data = "foo".as_bytes();
        mount
            .add(&PathBuf::from("/foo/bar"), data, None, true)
            .await
            .unwrap();

        let data = "bang".as_bytes();
        mount
            .add(&PathBuf::from("/foo/bug"), data, None, true)
            .await
            .unwrap();
    }
}
