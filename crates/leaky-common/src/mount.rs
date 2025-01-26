use std::collections::HashMap;
use std::convert::TryFrom;
use std::io::Read;
use std::ops::{Deref, DerefMut};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

use crate::ipfs_rpc::{IpfsRpc, IpfsRpcError};
use crate::types::{ipld_to_cid, NodeError, Object};
use crate::types::NodeLink;
use crate::types::Schema;
use crate::types::{Cid, Manifest, Node, Ipld};

// NOTE: this is really just used as a node cache, but right now it has some
//  mixed responsibilities
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

pub fn clean_path(path: &Path) -> PathBuf {
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

        Self::pull_nodes(manifest.data(), &block_cache, Some(ipfs_rpc)).await?;

        Ok(Self {
            cid,
            manifest: Arc::new(Mutex::new(manifest)),
            block_cache,
            ipfs_rpc: ipfs_rpc.clone(),
        })
    }

    pub async fn refresh(&mut self, cid: Cid) -> Result<(), MountError> {
        let ipfs_rpc = &self.ipfs_rpc;
        let manifest = Self::get::<Manifest>(&cid, ipfs_rpc).await?;
        let block_cache = Arc::new(Mutex::new(BlockCache::default()));

        Self::pull_nodes(manifest.data(), &block_cache, Some(ipfs_rpc)).await?;

        self.manifest = Arc::new(Mutex::new(manifest));
        self.block_cache = block_cache;

        Ok(())
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
        path: &Path,
        data: Option<(R, bool)>,
        object: Option<&Object>,
        schema: Option<Schema>,
    ) -> Result<(), MountError>
    where
        R: Read + Send + Sync + 'static + Unpin,
    {
        let ipfs_rpc = &self.ipfs_rpc;
        let block_cache = &self.block_cache;
        let path = clean_path(path);

        let link = match data {
            Some((d, true)) => {
                let cid = Self::hash_data(d, ipfs_rpc).await?;
                Some(cid)
            }
            Some((d, false)) => {
                let cid = Self::add_data(d, ipfs_rpc).await?;
                Some(cid)
            }
            None => None,
        };

        let data_node_cid = *self.manifest.lock().data();
        let mut node = Self::get_cache::<Node>(&data_node_cid, block_cache).await?;
        let consumed_path = PathBuf::from("/");
        let remaining_path = path;

        let maybe_new_data_node_cid =
            Self::upsert_node(
                &mut node,
                &consumed_path,
                &remaining_path,
                link,
                object,
                schema.map(|s| (s, true)),
                block_cache,
            )
            .await?;

        // if a change occurred, update the manifest and the cid
        if let Some(new_data_node_cid) = maybe_new_data_node_cid {
            self.manifest.lock().set_data(new_data_node_cid);
            let manifest = self.manifest.lock().clone();
            self.cid = Self::put::<Manifest>(&manifest, ipfs_rpc).await?;
        }

        // NOTE: this is kinda arbitray, we probably would be fine with OK(())
        // for now return the cid direct to the link
        Ok(())
    }

    pub async fn rm(&mut self, path: &Path) -> Result<(), MountError> {
        let ipfs_rpc = &self.ipfs_rpc;
        let block_cache = &self.block_cache;
        let path = clean_path(path);

        let data_node_cid = *self.manifest.lock().data();
        let mut node = Self::get_cache::<Node>(&data_node_cid, block_cache).await?;
        let consumed_path = PathBuf::from("/");
        let remaining_path = path;
        let maybe_new_data_node_cid = Self::upsert_node(
            &mut node,
            &consumed_path,
            &remaining_path,
            None,
            None,
            None,
            block_cache,
        )
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

    pub async fn ls(&self, path: &Path) -> Result<Vec<(String, NodeLink)>, MountError> {
        let block_cache = &self.block_cache;
        let path = clean_path(path);
        let data_node_cid = *self.manifest.lock().data();
        let mut node = Self::get_cache::<Node>(&data_node_cid, block_cache).await?;

        for part in path.iter() {
            let next = part.to_string_lossy().to_string();
            let next_link = node
                .get_link(&next)
                .ok_or(MountError::PathNotDir(path.clone()))?;
            node = Self::get_cache::<Node>(next_link.cid(), block_cache)
                .await
                .map_err(|_| MountError::PathNotDir(path.clone()))?;
        }

        let links: Vec<_> = node
            .get_links()
            .iter()
            .map(|(name, link)| (name.clone(), link.clone()))
            .collect();

        Ok(links)
    }

    pub async fn objects(&self) -> Result<Vec<(PathBuf, NodeLink)>, MountError> {
        let mut sorted_items = self.ls_deep(&PathBuf::from("/"), true).await?;
        sorted_items.sort_by(|a, b| a.0.cmp(&b.0));
        Ok(sorted_items)
    }

    pub async fn cat(&self, path: &Path) -> Result<Vec<u8>, MountError> {
        let ipfs_rpc = &self.ipfs_rpc;
        let block_cache = &self.block_cache;

        let path = clean_path(path);
        let data_node_cid = *self.manifest.lock().data();
        let mut node = Self::get_cache::<Node>(&data_node_cid, block_cache).await?;

        let dir_path = path.parent().unwrap_or(Path::new("/"));
        let file_name = path.file_name().unwrap().to_string_lossy().to_string();

        for part in dir_path.iter() {
            let next = part.to_string_lossy().to_string();
            let next_cid = node
                .get_link(&next)
                .ok_or(MountError::PathNotFile(path.clone()))?;
            node = Self::get_cache::<Node>(next_cid.cid(), block_cache).await?;
        }

        let link = node
            .get_link(&file_name)
            .ok_or(MountError::PathNotFile(path.clone()))?;
        let data = Self::cat_data(link.cid(), ipfs_rpc).await?;

        Ok(data)
    }

    #[async_recursion::async_recursion]
    async fn ls_deep(
        &self,
        path: &Path,
        objects_only: bool,
    ) -> Result<Vec<(PathBuf, NodeLink)>, MountError> {
        let mut items = vec![];
        let links = match self.ls(path).await {
            Ok(l) => l,
            Err(MountError::PathNotDir(_)) => return Ok(items),
            Err(err) => return Err(err),
        };

        for (name, link) in links {
            let mut current_path = path.to_path_buf();
            current_path.push(&name);

            if let NodeLink::Data(cid, object) = link {
                if !(objects_only && object.is_some()) {
                    items.push((current_path, NodeLink::Data(cid, object)));
                }
            } else {
                let mut _items = self.ls_deep(&current_path, objects_only).await?;
                items.append(&mut _items);
            }
        }

        Ok(items)
    }

    #[async_recursion::async_recursion]
    async fn pull_nodes(
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

        // Iterate over links using get_links()
        for (_, link) in node.get_links().iter() {
            match link {
                // NOTE: recurse over just node links
                NodeLink::Node(cid) => Self::pull_nodes(cid, block_cache, ipfs_rpc).await?,
                // NOTE: ignore data links
                _ => (),
            }
        }

        Ok(())
    }

    /// recursive upsert of a link into a node
    ///
    /// returns:
    ///  - None if no changes were made -- (removing something that dne)
    ///  - Some(Cid::default()) if the node was removed
    ///  - Some(cid) if the node was upserted
    #[async_recursion::async_recursion]
    async fn upsert_node(
        node: &mut Node,
        // keep track of the path we've already traversed
        consumed_path: &Path,
        // the path we're trying to upsert
        remaining_path: &Path,
        // set to None to remove the link
        maybe_link: Option<Cid>,
        // set an object to upsert
        maybe_object: Option<&Object>,
        // NOTE: you can only persist schemas on nodes, this argument 
        //  is mostly a way to carry over the schema from the parent node
        //  In order to persist a schema on a node you must set both the
        //  link and the object to None and set the schema to persist
        // set an schema to apply and whether to persist it
        maybe_schema: Option<(Schema, bool)>,
        block_cache: &Arc<Mutex<BlockCache>>,
    ) -> Result<Option<Cid>, MountError> {
        // determine if this is a rm or upsert (shouldn't really matter what schema is here)
        let is_rm = maybe_link.is_none() && maybe_object.is_none();
        // get the next link to follow
        let next_link = remaining_path
            .iter()
            .next()
            .unwrap()
            .to_string_lossy()
            .to_string();
        // NOTE: i dont like this behavior but its what we have for now
        // child schemas take precedence over parent schemas
        // -- unless we're persisting to a child
        let schema = match node.schema().cloned() {
            Some(s) => Some((s, false)),
            None => maybe_schema
                .as_ref()
                .map(|(s, persist)| (s.clone(), *persist)),
        };

        // determine where we are in the path
        match remaining_path.iter().count() {
            // this should never happen
            0 => panic!("path is empty"),
            // if this is the last part of the path, we need to upsert the link
            1 => {
                // NOTE this effectively doesnt happen since logically
                //  we should be able to set schemas and then upser a link
                // if the schema is not none, and we're here, then we're
                //  overwriting the schema on the node
                // match maybe_schema {
                //     Some((schema, true)) => {
                //         // upsert the schema and set it on the node
                //         node.set_schema(schema);
                //     }
                //     _ => (),
                // };
                // let schema = node.schema().cloned();

                // if this is a rm, we need to remove the link
                if is_rm {
                    let link = node.del(&next_link);
                    // double check that the link is there
                    // if nothing was removed, return None
                    if link.is_none() {
                        return Ok(None);
                    }
                    // otherwise if we removed the last link, then we need to
                    //  return the default cid
                    else if node.size() == 0 {
                        return Ok(Some(Cid::default()));
                    }
                }
                // NOTE: this being true means that schemas don't get persisted
                //  even if configured to do so
                else if let Some(link) = maybe_link {
                    // otherwise, upser the link
                    node.put_link(&next_link, link)?;
                }
                // otherwise, we need to handle edge cases where:
                // -- either we're upserting a schema into a child node
                // -- we potentially might be upserting metadata on a data link that
                //    doesn't exist
                else {
                    // so check if the link exists 
                    match node.get_link(&next_link) {
                        // if we have a node
                        Some(NodeLink::Node(cid)) => {
                            // and our schema is set to persist
                            if let Some((schema, true)) = schema.as_ref() {
                                // upsert the schema
                                let schema = schema.clone();
                                let mut next_node =
                                    Self::get_cache::<Node>(cid, block_cache).await?;
                                next_node.set_schema(schema);
                                // and put the node back in the cache
                                let next_node_cid =
                                    Self::put_cache::<Node>(&next_node, block_cache).await?;
                                // and update the link in the parent node
                                node.put_link(&next_link, next_node_cid)?;
                            }
                        }
                        // we have either noth
                        Some(NodeLink::Data(_, _)) => {
                            // if we're not setting an object, we need to error out
                            if maybe_object.is_none() {
                                return Err(MountError::Default(anyhow::anyhow!(
                                    "cannot set a schema on a data node: {}/{}",
                                    consumed_path.display(),
                                    next_link
                                )));
                            }
                        }
                        None => return Ok(None),
                    }
                }

                // if we have an object
                // upsert the object -- we should know that it always exists at this point
                if let Some(object) = maybe_object {
                    let _schema = schema.map(|(s, _)| s);
                    node.put_object(&next_link, object, _schema)?;
                }

                // and if we made it here, we need to put the node in the cache
                //  and bubble up the new cid
                let cid = Self::put_cache::<Node>(node, block_cache).await?;
                Ok(Some(cid))
            }
            // if this is not the last part of the path, we need to recurse
            //  by splitting off the path and recursing on the next node
            _ => {
                // update the paths
                let consumed_path = consumed_path.join(&next_link);
                let remaining_path = remaining_path.iter().skip(1).collect::<PathBuf>();
                
                // get the next link
                let mut next_node = match node.get_link(&next_link) {
                    // if we've run into a data node, we need to error out -- there's no where else to traverse
                    Some(NodeLink::Data(_, _)) => {
                        // this should never happen
                        return Err(MountError::Default(anyhow::anyhow!(
                            "data node encountered at path: {}/{}",
                            consumed_path.display(),
                            next_link
                        )));
                    }
                    // if we've run into a node, we need to recurse on it
                    Some(NodeLink::Node(cid)) => {
                        // get the next cid by looking up the link
                        let next_node_cid = *cid;
                        // and get the node from the cache
                        Self::get_cache::<Node>(&next_node_cid, block_cache).await?
                    }
                    // otherwise
                    None => {
                        // if this is creating a new link, we need to create a new node
                        if maybe_link.is_some() {
                            let new_node = Node::default();
                            Self::put_cache::<Node>(&new_node, block_cache).await?;
                            new_node
                        }
                        // otherwise, this means we're trying to update an object at a link
                        //  that doesn't exist
                        //  -- this should never happen in the client
                        else {
                            return Ok(None);
                        }
                    }
                };

                // recurse on the next node
                let maybe_cid = Self::upsert_node(
                    &mut next_node,
                    &consumed_path,
                    &remaining_path,
                    maybe_link,
                    maybe_object,
                    maybe_schema,
                    block_cache,
                )
                .await?;

                match maybe_cid {
                    // if the next node was removed, we need to remove the link
                    Some(cid) if cid == Cid::default() => {
                        node.del(&next_link);
                        if node.size() == 0 {
                            // if this was the last link, then prune the node
                            //  by returning the default cid
                            return Ok(Some(Cid::default()));
                        }
                    }
                    // otherwise, upsert the link
                    Some(cid) => {
                        node.put_link(&next_link, cid)?;
                    }
                    // otherwise signal no changes were made
                    None => return Ok(None),
                }

                // and if we made it here, we need to put the node in the cache
                //  and bubble up the new cid
                let cid = Self::put_cache::<Node>(node, block_cache).await?;
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
        let cid = ipfs_rpc.hash_data(data).await?;
        Ok(cid)
    }

    pub async fn add_data<R>(data: R, ipfs_rpc: &IpfsRpc) -> Result<Cid, MountError>
    where
        R: Read + Send + Sync + 'static + Unpin,
    {
        let cid = ipfs_rpc.add_data(data).await?;
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
        let ipld = ipfs_rpc.get_ipld(cid).await?;
        let object = B::try_from(ipld).map_err(|_| MountError::Ipld)?;
        Ok(object)
    }

    async fn put<B>(ipld: &B, ipfs_rpc: &IpfsRpc) -> Result<Cid, MountError>
    where
        B: Into<Ipld> + Clone,
    {
        let cid = ipfs_rpc.put_ipld(ipld.clone()).await?;
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

    async fn put_cache<B>(ipld: &B, block_cache: &Arc<Mutex<BlockCache>>) -> Result<Cid, MountError>
    where
        B: Into<Ipld> + Clone,
    {
        // convert our ipld able thing to a block
        //  in order to determine the cid
        let ipld: Ipld = ipld.clone().into();
        let cid = ipld_to_cid(ipld.clone());

        block_cache.lock().insert(cid.to_string(), ipld);
        Ok(cid)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum MountError {
    #[error("default error: {0}")]
    Default(#[from] anyhow::Error),
    #[error("block cache miss: {0}")]
    BlockCacheMiss(Cid),
    #[error("node error: {0}")]
    Node(#[from] NodeError),
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
        Mount::init(&ipfs_rpc).await.unwrap()
    }

    #[tokio::test]
    async fn add() {
        let mut mount = empty_mount().await;
        let data = "foo".as_bytes();
        mount
            .add(&PathBuf::from("/foo"), Some((data, true)), None, None)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn add_with_metadata() {
        let mut mount = empty_mount().await;
        let data = "foo".as_bytes();
        let mut object = Object::default();
        object.insert("foo".to_string(), Ipld::String("bar".to_string()));
        mount
            .add(&PathBuf::from("/foo"), Some((data, true)), Some(&object), None)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn add_cat() {
        let mut mount = empty_mount().await;
        let data = "foo".as_bytes();
        mount
            .add(&PathBuf::from("/bar"), Some((data, false)), None, None)
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
            .add(&PathBuf::from("/bar"), Some((data, true)), None, None)
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
            .add(&PathBuf::from("/foo/bar/buzz"), Some((data, true)), None, None)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn add_rm() {
        let mut mount = empty_mount().await;
        let data = "foo".as_bytes();
        mount
            .add(&PathBuf::from("/foo/bar"), Some((data, true)), None, None)
            .await
            .unwrap();
        mount.rm(&PathBuf::from("/foo/bar")).await.unwrap();
    }

    #[tokio::test]
    async fn add_pull_ls() {
        let mut mount = empty_mount().await;
        let data = "foo".as_bytes();
        mount
            .add(&PathBuf::from("/bar"), Some((data, true)), None, None)
            .await
            .unwrap();
        let cid = *mount.cid();
        mount.push().await.unwrap();

        let mount = Mount::pull(cid, &IpfsRpc::default()).await.unwrap();
        assert_eq!(mount.ls(&PathBuf::from("/")).await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn add_add_deep() {
        let mut mount = empty_mount().await;

        let data = "foo".as_bytes();
        mount
            .add(&PathBuf::from("/foo/bar"), Some((data, true)), None, None)
            .await
            .unwrap();

        let data = "bang".as_bytes();
        mount
            .add(&PathBuf::from("/foo/bug"), Some((data, true)), None, None)
            .await
            .unwrap();
    }
}
