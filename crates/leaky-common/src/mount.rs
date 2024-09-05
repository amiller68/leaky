use std::collections::BTreeMap;
use std::collections::HashMap;
use std::convert::TryFrom;
use std::io::Read;
use std::ops::{Deref, DerefMut};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use parking_lot::Mutex;
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

pub fn clean_path(path: &PathBuf) -> PathBuf {
    if !path.is_absolute() {
        panic!("path is not absolute");
    }

    path.iter()
        .skip(1)
        .map(|part| part.to_string_lossy().to_string())
        .collect::<PathBuf>()
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
        self.manifest.lock().clone()
    }

    pub fn block_cache(&self) -> BlockCache {
        self.block_cache.lock().clone()
    }

    pub async fn init(ipfs_rpc: &IpfsRpc) -> Result<Self, MountError> {
        let mut manifest = Manifest::default();
        let block_cache = Arc::new(Mutex::new(BlockCache::default()));
        let node = Node::default();
        let data_cid = Self::put_cache::<Node>(&node, &block_cache).await?;
        manifest.set_data(data_cid);
        let cid = Self::put::<Manifest>(&manifest, ipfs_rpc).await?;

        Ok(Self {
            cid,
            manifest: Arc::new(Mutex::new(manifest)),
            block_cache,
            ipfs_rpc: ipfs_rpc.clone(),
        })
    }

    pub async fn pull(cid: Cid, ipfs_rpc: &IpfsRpc) -> Result<Self, MountError> {
        let manifest = Self::get::<Manifest>(&cid, ipfs_rpc).await?;
        let block_cache = Arc::new(Mutex::new(BlockCache::default()));

        Self::pull_links(manifest.data(), &block_cache, Some(ipfs_rpc)).await?;

        Ok(Self {
            cid,
            manifest: Arc::new(Mutex::new(manifest)),
            block_cache,
            ipfs_rpc: ipfs_rpc.clone(),
        })
    }

    pub async fn push(&mut self) -> Result<(), MountError> {
        let ipfs_rpc = &self.ipfs_rpc;
        let block_cache_data = self.block_cache.lock().clone();

        for (cid_str, ipld) in block_cache_data.iter() {
            let cid = Self::put::<Ipld>(ipld, ipfs_rpc).await?;
            assert_eq!(cid.to_string(), cid_str.to_string());
        }

        let manifest = self.manifest.lock().clone();
        self.cid = Self::put::<Manifest>(&manifest, ipfs_rpc).await?;

        Ok(())
    }

    // TODO: this is janky, but fixes the fact that we don't cache anything
    //  locally and improperly update the previoud cid
    pub fn set_previous(&mut self, previous: Cid) {
        self.manifest.lock().set_previous(previous);
    }

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

        let data_cid = if hash_only {
            Self::hash_data(data, ipfs_rpc).await?
        } else {
            Self::add_data(data, ipfs_rpc).await?
        };

        let data_node_cid = self.manifest.lock().data().clone();

        let maybe_new_data_node_cid = Self::upsert_link_and_object(
            &data_node_cid,
            &path,
            Some(&data_cid),
            maybe_metadata,
            ipfs_rpc,
            block_cache,
        )
        .await?;

        if let Some(new_data_node_cid) = maybe_new_data_node_cid {
            self.manifest.lock().set_data(new_data_node_cid);
            let manifest = self.manifest.lock().clone();
            self.cid = Self::put::<Manifest>(&manifest, ipfs_rpc).await?;
        }

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

        let data_node_cid = self.manifest.lock().data().clone();
        let maybe_new_data_node_cid = Self::upsert_link_and_object(
            &data_node_cid,
            &path,
            None,
            Some(metadata),
            ipfs_rpc,
            block_cache,
        )
        .await?;

        if let Some(new_data_node_cid) = maybe_new_data_node_cid {
            self.manifest.lock().set_data(new_data_node_cid);
            let manifest = self.manifest.lock().clone();
            self.cid = Self::put::<Manifest>(&manifest, ipfs_rpc).await?;
        }

        Ok(())
    }

    pub async fn rm(&mut self, path: &PathBuf) -> Result<(), MountError> {
        let ipfs_rpc = &self.ipfs_rpc;
        let block_cache = &self.block_cache;
        let path = clean_path(path);

        let data_node_cid = self.manifest.lock().data().clone();
        let maybe_new_data_node_cid =
            Self::upsert_link_and_object(&data_node_cid, &path, None, None, ipfs_rpc, block_cache)
                .await?;

        if let Some(new_data_node_cid) = maybe_new_data_node_cid {
            let new_data_node_cid = if new_data_node_cid == Cid::default() {
                let data_node = Node::default();
                Self::put_cache::<Node>(&data_node, block_cache).await?
            } else {
                new_data_node_cid
            };

            self.manifest.lock().set_data(new_data_node_cid);
            let manifest = self.manifest.lock().clone();
            self.cid = Self::put::<Manifest>(&manifest, ipfs_rpc).await?;
        }

        Ok(())
    }

    pub async fn ls(
        &self,
        path: &PathBuf,
    ) -> Result<Vec<(String, (Cid, Option<Object>))>, MountError> {
        let block_cache = &self.block_cache;
        let path = clean_path(path);
        let data_node_cid = self.manifest.lock().data().clone();
        let mut node = Self::get_cache::<Node>(&data_node_cid, block_cache).await?;

        for part in path.iter() {
            let next = part.to_string_lossy().to_string();
            let next_cid = node
                .get_link(&next)
                .ok_or(MountError::PathNotDir(path.clone()))?;
            node = Self::get_cache::<Node>(&next_cid, block_cache)
                .await
                .map_err(|_| MountError::PathNotDir(path.clone()))?;
        }

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

    pub async fn items(&self) -> Result<Vec<(PathBuf, Cid)>, MountError> {
        let mut sorted_items = self.recursive_items(&PathBuf::from("/")).await?;
        sorted_items.sort_by(|a, b| a.0.cmp(&b.0));
        Ok(sorted_items)
    }

    pub async fn cat(&self, path: &PathBuf) -> Result<Vec<u8>, MountError> {
        let ipfs_rpc = &self.ipfs_rpc;
        let block_cache = &self.block_cache;

        let path = clean_path(path);
        let data_node_cid = self.manifest.lock().data().clone();
        let mut node = Self::get_cache::<Node>(&data_node_cid, block_cache).await?;

        let dir_path = path.parent().unwrap_or(Path::new("/"));
        let file_name = path.file_name().unwrap().to_string_lossy().to_string();

        for part in dir_path.iter() {
            let next = part.to_string_lossy().to_string();
            let next_cid = node
                .get_link(&next)
                .ok_or(MountError::PathNotFile(path.clone()))?;
            node = Self::get_cache::<Node>(&next_cid, block_cache).await?;
        }

        let link = node
            .get_link(&file_name)
            .ok_or(MountError::PathNotFile(path.clone()))?;
        let data = Self::cat_data(&link, ipfs_rpc).await?;

        Ok(data)
    }

    #[async_recursion::async_recursion]
    async fn recursive_items(&self, path: &PathBuf) -> Result<Vec<(PathBuf, Cid)>, MountError> {
        let mut items = vec![];
        let links = match self.ls(path).await {
            Ok(l) => l,
            Err(MountError::PathNotDir(_)) => return Ok(items),
            Err(err) => return Err(err),
        };

        for (name, (_link, object)) in links {
            let mut current_path = path.clone();
            current_path.push(&name);

            if object.is_none() {
                let mut next_items = self.recursive_items(&current_path).await?;
                items.append(&mut next_items);
            } else {
                items.push((current_path, _link));
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
            Self::get::<Node>(cid, ipfs_rpc).await?
        } else {
            Self::get_cache::<Node>(cid, block_cache).await?
        };
        block_cache
            .lock()
            .insert(cid.to_string(), node.clone().into());

        for (_name, link) in node.iter() {
            if let Ipld::Link(cid) = link {
                if cid.codec() != 0x55 {
                    Self::pull_links(cid, block_cache, ipfs_rpc).await?;
                }
            }
        }

        Ok(())
    }

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
        let mut node = Self::get_cache::<Node>(cid, block_cache).await?;
        let next = path.iter().next().unwrap().to_string_lossy().to_string();

        match path.iter().count() {
            0 => panic!("path is empty"),
            1 => {
                if is_rm {
                    if node.del(&next).0.is_none() {
                        return Ok(None);
                    }
                    if node.size() == 0 {
                        return Ok(Some(Cid::default()));
                    }
                } else {
                    node.update_link(&next, maybe_link, maybe_metadata);
                }

                let cid = Self::put_cache::<Node>(&node, block_cache).await?;
                Ok(Some(cid))
            }
            _ => {
                let remaining = path.iter().skip(1).collect::<PathBuf>();
                let next_cid = if let Some(next_cid) = node.get_link(&next) {
                    next_cid
                } else if !is_rm {
                    let new_node = Node::default();
                    Self::put_cache::<Node>(&new_node, block_cache).await?
                } else {
                    return Ok(None);
                };

                let maybe_cid = Self::upsert_link_and_object(
                    &next_cid,
                    &remaining,
                    maybe_link,
                    maybe_metadata,
                    ipfs_rpc,
                    block_cache,
                )
                .await?;

                match maybe_cid {
                    Some(cid) if cid == Cid::default() => {
                        node.del(&next);
                        if node.size() == 0 {
                            return Ok(Some(Cid::default()));
                        }
                    }
                    Some(cid) => {
                        node.put_link(&next, &cid);
                    }
                    None => return Ok(None),
                }

                let cid = Self::put_cache::<Node>(&node, block_cache).await?;
                Ok(Some(cid))
            }
        }
    }

    pub async fn _hash_data<R>(&self, data: R) -> Result<Cid, MountError>
    where
        R: Read + Send + Sync + 'static + Unpin,
    {
        Self::hash_data(data, &self.ipfs_rpc).await
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
        let block =
            Block::<DefaultParams>::new(*cid, data).map_err(|_| MountError::BlockCreation)?;
        let ipld = block
            .decode::<DagCborCodec, Ipld>()
            .map_err(|_| MountError::BlockDecode)?;
        let object = B::try_from(ipld).map_err(|_| MountError::Ipld)?;
        Ok(object)
    }

    async fn put<B>(object: &B, ipfs_rpc: &IpfsRpc) -> Result<Cid, MountError>
    where
        B: Into<Ipld> + Clone,
    {
        let ipld: Ipld = object.clone().into();
        let block = Block::<DefaultParams>::encode(DagCborCodec, MhCode::Blake3_256, &ipld)
            .map_err(|_| MountError::BlockEncoding)?;
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
        let cid_str = cid.to_string();
        let ipld = block_cache
            .lock()
            .get(&cid_str)
            .cloned()
            .ok_or(MountError::BlockCacheMiss(*cid))?;
        let object = B::try_from(ipld).map_err(|_| MountError::Ipld)?;
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
        .map_err(|_| MountError::BlockEncoding)?;
        let cid = *block.cid();

        block_cache
            .lock()
            .insert(cid.to_string(), object.clone().into());
        Ok(cid)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum MountError {
    #[error("block cache miss: {0}")]
    BlockCacheMiss(Cid),
    #[error("ipfs rpc error: {0}")]
    IpfsRpc(#[from] IpfsRpcError),
    #[error("could not convert Ipld to type")]
    Ipld,
    #[error("cid is not set")]
    NoCid,
    #[error("path is not directory: {0}")]
    PathNotDir(PathBuf),
    #[error("path is not file: {0}")]
    PathNotFile(PathBuf),
    #[error("block creation failed")]
    BlockCreation,
    #[error("block decoding failed")]
    BlockDecode,
    #[error("block encoding failed")]
    BlockEncoding,
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
        assert_eq!(data, get_data.as_slice());
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
        let cid = mount.cid().clone();
        mount.push().await.unwrap();

        let mount = Mount::pull(cid, &IpfsRpc::default()).await.unwrap();
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
