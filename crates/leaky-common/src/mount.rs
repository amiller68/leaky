use std::collections::{BTreeMap, HashMap};
use std::convert::TryFrom;
use std::io::Read;
use std::ops::{Deref, DerefMut};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

use crate::ipfs_rpc::{IpfsRpc, IpfsRpcError};
use crate::types::NodeLink;
use crate::types::Schema;
use crate::types::{ipld_to_cid, NodeError, Object};
use crate::types::{Cid, Ipld, Manifest, Node};

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

// NOTE: the mount api requires absolute paths, but we transalte those into
//  relative paths to make iteration slightly easier
// Kinda janky but it works for now
pub fn clean_path(path: &Path) -> PathBuf {
    if !path.is_absolute() {
        panic!("path is not absolute");
    }

    path.iter()
        .skip(1)
        .map(|part| part.to_string_lossy().to_string())
        .collect::<PathBuf>()
}

// TODO: ipfs rpc and block cache should not be apart of the mount struct
//  they are less state than injectable dependencies
#[derive(Clone)]
pub struct Mount {
    cid: Cid,
    manifest: Arc<Mutex<Manifest>>,
    block_cache: Arc<Mutex<BlockCache>>,
    ipfs_rpc: IpfsRpc,
}

impl Mount {
    // getters

    pub fn cid(&self) -> &Cid {
        &self.cid
    }

    pub fn manifest(&self) -> Manifest {
        self.manifest.lock().clone()
    }

    pub fn previous_cid(&self) -> Cid {
        *self.manifest.lock().previous()
    }

    pub fn block_cache(&self) -> BlockCache {
        self.block_cache.lock().clone()
    }

    // setters

    pub fn set_previous(&mut self, previous: Cid) {
        self.manifest.lock().set_previous(previous);
    }

    // mount sync

    /// Initialize a fresh mount against a given ipfs rpc
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

    /// Pull a mount from a given ipfs rpc using its root cid
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

    /// update an existing mount against an updated ipfs rpc
    pub async fn update(&mut self, cid: Cid) -> Result<(), MountError> {
        let manifest = Self::get::<Manifest>(&cid, &self.ipfs_rpc).await?;
        println!("UPDATE MANIFEST: {:?}", manifest);
        // make sure the mount points back to our current cid
        let previous_cid = manifest.previous();
        if *previous_cid != self.cid {
            return Err(MountError::PreviousCidMismatch(*previous_cid, self.cid));
        }
        // purge the block cache
        let block_cache = Arc::new(Mutex::new(BlockCache::default()));
        // pull the nodes
        Self::pull_nodes(manifest.data(), &block_cache, Some(&self.ipfs_rpc)).await?;
        // update the manifest and block cache
        self.manifest = Arc::new(Mutex::new(manifest));
        self.block_cache = block_cache;
        self.cid = cid;
        Ok(())
    }

    /// push state against our ipfs rpc
    pub async fn push(&mut self) -> Result<(), MountError> {
        let ipfs_rpc = &self.ipfs_rpc;
        let block_cache_data = self.block_cache.lock().clone();
        // iterate through the block cache and push each block in the cache
        for (cid_str, ipld) in block_cache_data.iter() {
            let cid = Self::put::<Ipld>(ipld, ipfs_rpc).await?;
            assert_eq!(cid.to_string(), cid_str.to_string());
        }

        let manifest = self.manifest.lock().clone();
        self.cid = Self::put::<Manifest>(&manifest, ipfs_rpc).await?;

        Ok(())
    }

    // mount operations api

    /// add or upsert data at a given path within the mount.
    ///  Does and should not handle inserting object or schema
    ///  metadata into the mount.
    ///
    /// # Arguments
    ///
    /// * `path` - the path to add the data at
    /// * `(data, hash_only)` - the data to add and a flag to indicate if we should write
    ///     the data to ipfs or just hash it
    ///
    /// # Returns
    ///
    /// * `Ok(())` - if the data was added successfully
    /// * `Err(MountError)` - if the data could not be added
    pub async fn add<R>(&mut self, path: &Path, data: (R, bool)) -> Result<(), MountError>
    where
        R: Read + Send + Sync + 'static + Unpin,
    {
        let ipfs_rpc = &self.ipfs_rpc;
        let block_cache = &self.block_cache;
        // always clean the path
        let path = clean_path(path);

        // get a cid link to insert regardles of if we are hashing or not
        let link = match data {
            (d, true) => {
                let cid = Self::hash_data(d, ipfs_rpc).await?;
                Some(cid)
            }
            (d, false) => {
                let cid = Self::add_data(d, ipfs_rpc).await?;
                Some(cid)
            }
        };

        // get our entry into the mount
        let data_node_cid = *self.manifest.lock().data();
        let mut node = Self::get_cache::<Node>(&data_node_cid, block_cache).await?;

        // keep track of our consumed path and remaining path
        let consumed_path = PathBuf::from("/");
        let remaining_path = path;

        // and upsert the node -- we'll get a cid back if the tree changed
        // NOTE: we don't have to handle Cid::default() here because we know
        //  that nothing gets removed from the mount unless we set `link` to None
        let maybe_new_data_node_cid = Self::upsert_node(
            &mut node,
            &consumed_path,
            &remaining_path,
            link,
            None,
            None,
            block_cache,
        )
        .await?;

        // if a change occurred, update the manifest and the cid
        if let Some(new_data_node_cid) = maybe_new_data_node_cid {
            self.manifest.lock().set_data(new_data_node_cid);
            let manifest = self.manifest.lock().clone();
            self.cid = Self::put::<Manifest>(&manifest, ipfs_rpc).await?;
        }

        Ok(())
    }

    /// remove data or node at a given path within the mount
    ///  Will remove objects and schemas at the given path
    ///  if removing a node
    ///
    /// # Arguments
    ///
    /// * `path` - the path to remove the data at
    ///
    /// # Returns
    ///
    /// * `Ok(())` - if the data was removed successfully
    /// * `Err(MountError)` - if the data could not be removed
    pub async fn rm(&mut self, path: &Path) -> Result<(), MountError> {
        let ipfs_rpc = &self.ipfs_rpc;
        let block_cache = &self.block_cache;
        // always clean the path
        let path = clean_path(path);

        // get our entry into the mount
        let data_node_cid = *self.manifest.lock().data();
        let mut node = Self::get_cache::<Node>(&data_node_cid, block_cache).await?;

        // keep track of our consumed path and remaining path
        let consumed_path = PathBuf::from("/");
        let remaining_path = path;

        // and remove the target node or link
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

        // if a change occurred, update the manifest and the cid
        if let Some(new_data_node_cid) = maybe_new_data_node_cid {
            // if the new data node cid is default, then the data node was removed
            //  or otherwise cleaned up (nodes must hold at least one child). we
            //  need to create a new default node and upsert it into the mount
            // otherwise we need to insert the updated data node.
            let new_data_node_cid = if new_data_node_cid == Cid::default() {
                let data_node = Node::default();
                Self::put_cache::<Node>(&data_node, block_cache).await?
            } else {
                new_data_node_cid
            };

            // update the manifest and the cid
            self.manifest.lock().set_data(new_data_node_cid);
            let manifest = self.manifest.lock().clone();
            self.cid = Self::put::<Manifest>(&manifest, ipfs_rpc).await?;
        }

        Ok(())
    }

    pub async fn ls(
        &self,
        path: &Path,
        deep: bool,
    ) -> Result<(BTreeMap<PathBuf, NodeLink>, Option<Schema>), MountError> {
        // always clean the path
        let path = clean_path(path);

        // get the node at the path
        let node_link = self.get_node_link_at_path(&path).await?;
        match node_link {
            NodeLink::Data(_, _) => Err(MountError::PathNotDir(path.to_path_buf())),
            NodeLink::Node(cid) => {
                if deep {
                    let node = Self::get_cache::<Node>(&cid, &self.block_cache).await?;
                    let items = self.ls_deep(&path, &node).await?;
                    Ok((items.into_iter().collect(), None))
                } else {
                    let node = Self::get_cache::<Node>(&cid, &self.block_cache).await?;

                    let schema = node.schema().cloned();
                    let links = node.get_links();
                    Ok((
                        links
                            .iter()
                            .map(|(k, v)| (PathBuf::from(k), v.clone()))
                            .collect(),
                        schema,
                    ))
                }
            }
        }
    }

    /// cat data at a given path within the mount
    ///  Does and should not handle getting object or schema
    ///  metadata from the mount
    ///
    /// # Arguments
    ///
    /// * `path` - the path to get the data at
    ///
    /// # Returns
    ///
    /// * `Ok(Vec<u8>)` - if the data was retrieved successfully
    /// * `Err(MountError)` - if the data could not be retrieved
    pub async fn cat(&self, path: &Path) -> Result<Vec<u8>, MountError> {
        println!("CAT {:?}", path);
        let ipfs_rpc = &self.ipfs_rpc;
        // always clean the path
        let path = clean_path(path);

        // get the node at the path
        let node_link = self.get_node_link_at_path(&path).await?;
        match node_link {
            NodeLink::Data(cid, _) => {
                let data = Self::cat_data(&cid, ipfs_rpc).await?;
                Ok(data)
            }
            NodeLink::Node(_) => Err(MountError::PathNotFile(path.to_path_buf())),
        }
    }

    /// Tag an object at a given path within the mount
    ///  with metadata
    ///
    ///  # Arguments
    ///  * `path` - the path to tag the object at
    ///  * `object` - the object to tag
    ///
    ///  # Returns
    ///  * `Ok(())` - if the object was tagged successfully
    ///  * `Err(MountError)` - if the object could not be tagged
    pub async fn tag(&mut self, path: &Path, object: Object) -> Result<(), MountError> {
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
            Some(&object),
            None,
            block_cache,
        )
        .await?;

        // if a change occurred, update the manifest and the cid
        if let Some(new_data_node_cid) = maybe_new_data_node_cid {
            self.manifest.lock().set_data(new_data_node_cid);
            let manifest = self.manifest.lock().clone();
            self.cid = Self::put::<Manifest>(&manifest, ipfs_rpc).await?;
        }

        Ok(())
    }

    /// add a schema at a given path within the mount
    ///  Does and should not handle inserting object or data into the mount
    ///
    /// # Arguments
    ///
    /// * `path` - the path to add the schema at
    /// * `schema` - the schema to add
    ///
    /// # Returns
    ///
    /// * `Ok(())` - if the schema was added successfully
    /// * `Err(MountError)` - if the schema could not be added
    pub async fn set_schema(&mut self, path: &Path, schema: Schema) -> Result<(), MountError> {
        let ipfs_rpc = &self.ipfs_rpc;
        let block_cache = &self.block_cache;
        // always clean the path
        let path = clean_path(path);

        // get our entry into the mount
        let data_node_cid = *self.manifest.lock().data();
        let mut node = Self::get_cache::<Node>(&data_node_cid, block_cache).await?;

        // keep track of our consumed path and remaining path
        let consumed_path = PathBuf::from("/");
        let remaining_path = path;

        // and upsert the node -- we'll get a cid back if the tree changed
        // NOTE: we don't have to handle Cid::default() here because we know
        //  that nothing gets removed from the mount unless we set `link` to None.
        //  At worst here we're just dropping a schema
        let maybe_new_data_node_cid = Self::upsert_node(
            &mut node,
            &consumed_path,
            &remaining_path,
            None,
            None,
            Some((schema, true)),
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

    /// Get a node at a given path
    ///  Nodes are returned if the path ends in a node.
    ///
    /// # Arguments
    ///
    /// * `path` - the path to get the node at
    ///
    /// # Returns
    ///
    /// * `Ok((node, node_link))` - if the node or node link was found
    /// * `Err(MountError)` - if the node or node link could not be found
    async fn get_node_link_at_path(&self, path: &Path) -> Result<NodeLink, MountError> {
        let block_cache = &self.block_cache;
        // path should be cleaned already
        println!("GET_NODE_LINK_AT_PATH {:?}", path);

        println!("MANIFEST: {:?}", self.manifest.lock());

        // get our entry into the mount
        let data_node_cid = *self.manifest.lock().data();

        println!("DATA_NODE_CID: {:?}", data_node_cid);
        // if this is just / then we're done
        if path.iter().count() == 0 {
            println!("RETURNING DATA NODE CID");
            return Ok(NodeLink::Node(data_node_cid));
        }

        let mut node = Self::get_cache::<Node>(&data_node_cid, block_cache).await?;
        println!("NODE: {:?}", node);
        // keep track of our consumed path and remaining path
        let mut consumed_path = PathBuf::from("/");
        let link_name = path
            .iter()
            .next_back()
            .unwrap()
            .to_string_lossy()
            .to_string();

        // get the path to the link
        // i.e. writing/path/assets -> writing/path
        let link_path = path
            .iter()
            .rev()
            .skip(1)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect::<PathBuf>();

        // iterate through the path and get the node at each step
        for part in link_path.iter() {
            consumed_path.push(part);
            let next = part.to_string_lossy().to_string();
            // get the next link
            let next_link = match node.get_link(&next) {
                Some(link) => link,
                None => {
                    return Err(MountError::PathNotFound(consumed_path.clone()));
                }
            };
            // get the next node from the cache
            node = match Self::get_cache::<Node>(next_link.cid(), block_cache).await {
                // this is just a node
                Ok(n) => n,
                Err(err) => match err {
                    // this was not a node
                    MountError::Ipld => {
                        return Err(MountError::PathNotNode(consumed_path.clone()));
                    }
                    // the path was not found
                    MountError::PathNotFound(_) => {
                        return Err(MountError::PathNotFound(consumed_path.clone()));
                    }
                    // otherwise
                    err => return Err(err),
                },
            };
        }

        // get the link at the end of the path
        let link = match node.get_link(&link_name) {
            Some(link) => link.clone(),
            None => {
                return Err(MountError::PathNotFound(consumed_path.clone()));
            }
        };

        // return the node
        Ok(link)
    }

    #[async_recursion::async_recursion]
    async fn ls_deep(
        &self,
        path: &Path,
        node: &Node,
    ) -> Result<BTreeMap<PathBuf, NodeLink>, MountError> {
        let mut items = BTreeMap::new();
        let links = node.get_links();

        for (name, link) in links {
            let mut current_path = path.to_path_buf();
            current_path.push(name);

            match link {
                NodeLink::Data(cid, object) => {
                    items.insert(current_path.clone(), NodeLink::Data(*cid, object.clone()));
                }

                NodeLink::Node(cid) => {
                    let node = Self::get_cache::<Node>(cid, &self.block_cache).await?;

                    let mut _items = self.ls_deep(&current_path, &node).await?;
                    items.append(&mut _items);
                }
            };
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
            if let NodeLink::Node(cid) = link {
                Self::pull_nodes(cid, block_cache, ipfs_rpc).await?;
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
        let is_rm = maybe_link.is_none() && maybe_object.is_none() && maybe_schema.is_none();
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
                        None => {
                            // if we're persisting a schema, we need to create a new node
                            if let Some((schema, true)) = schema.as_ref() {
                                let mut new_node = Node::default();
                                new_node.set_schema(schema.clone());
                                let new_node_cid =
                                    Self::put_cache::<Node>(&new_node, block_cache).await?;
                                node.put_link(&next_link, new_node_cid)?;
                            } else {
                                return Ok(None);
                            }
                        }
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
        B: TryFrom<Ipld> + std::fmt::Debug + Send,
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
    #[error("path not found: {0}")]
    PathNotFound(PathBuf),
    #[error("path is not a node: {0}")]
    PathNotNode(PathBuf),
    #[error("path is not a node link: {0}")]
    PathNotNodeLink(PathBuf),
    #[error("previous cid mismatch: {0} != {1}")]
    PreviousCidMismatch(Cid, Cid),
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
            .add(&PathBuf::from("/foo"), (data, true))
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
            .add(&PathBuf::from("/foo"), (data, true))
            .await
            .unwrap();
        mount.tag(&PathBuf::from("/foo"), object).await.unwrap();
    }

    #[tokio::test]
    async fn add_cat() {
        let mut mount = empty_mount().await;
        let data = "foo".as_bytes();
        mount
            .add(&PathBuf::from("/bar"), (data, false))
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
            .add(&PathBuf::from("/bar"), (data, true))
            .await
            .unwrap();
        let (links, _) = mount.ls(&PathBuf::from("/"), false).await.unwrap();
        assert_eq!(links.len(), 1);
    }

    #[tokio::test]
    async fn add_set_schema_ls() {
        let mut mount = empty_mount().await;
        let data = "foo".as_bytes();
        let schema = Schema::default();
        mount
            .add(&PathBuf::from("/bar/buz"), (data, true))
            .await
            .unwrap();
        mount
            .set_schema(&PathBuf::from("/bar"), schema)
            .await
            .unwrap();
        let (links, schema) = mount.ls(&PathBuf::from("/bar"), false).await.unwrap();
        assert_eq!(links.len(), 1);
        assert!(schema.is_some());
    }

    #[tokio::test]
    async fn set_schema_ls_nonexistant_path() {
        let mut mount = empty_mount().await;
        let schema = Schema::default();
        mount
            .set_schema(&PathBuf::from("/bar"), schema)
            .await
            .unwrap();
        let (_ls, schema) = mount.ls(&PathBuf::from("/bar"), false).await.unwrap();
        assert!(schema.is_some());
    }

    #[tokio::test]
    async fn add_deep() {
        let mut mount = empty_mount().await;
        let data = "foo".as_bytes();
        mount
            .add(&PathBuf::from("/foo/bar/buzz"), (data, true))
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn add_rm() {
        let mut mount = empty_mount().await;
        let data = "foo".as_bytes();
        mount
            .add(&PathBuf::from("/foo/bar"), (data, true))
            .await
            .unwrap();
        mount.rm(&PathBuf::from("/foo/bar")).await.unwrap();
    }

    #[tokio::test]
    async fn add_pull_ls() {
        let mut mount = empty_mount().await;
        let data = "foo".as_bytes();
        mount
            .add(&PathBuf::from("/bar"), (data, true))
            .await
            .unwrap();
        let cid = *mount.cid();
        mount.push().await.unwrap();

        let mount = Mount::pull(cid, &IpfsRpc::default()).await.unwrap();
        let (ls, _) = mount.ls(&PathBuf::from("/"), false).await.unwrap();
        assert_eq!(ls.len(), 1);
    }

    #[tokio::test]
    async fn add_add_deep() {
        let mut mount = empty_mount().await;

        let data = "foo".as_bytes();
        mount
            .add(&PathBuf::from("/foo/bar"), (data, true))
            .await
            .unwrap();

        let data = "bang".as_bytes();
        mount
            .add(&PathBuf::from("/foo/bug"), (data, true))
            .await
            .unwrap();
    }
}
