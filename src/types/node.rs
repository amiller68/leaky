use std::collections::BTreeMap;
use std::convert::TryFrom;
use std::ops::Deref;

use super::ipld::{Cid, Ipld};
use super::object::Object;

// Reserved metadata key for detailing what links
//  within have visible metatdata attached to them
const METADATA_KEY: &str = ".metadata";

#[derive(Debug, Clone, PartialEq)]
pub struct Node(BTreeMap<String, Ipld>);

impl Default for Node {
    fn default() -> Self {
        // Create an empty .metadata map
        let metadata = BTreeMap::new();
        let mut map = BTreeMap::new();
        map.insert(METADATA_KEY.to_string(), Ipld::Map(metadata));
        Self(map)
    }
}

impl From<Node> for Ipld {
    fn from(node: Node) -> Self {
        Ipld::Map(node.0)
    }
}

impl TryFrom<Ipld> for Node {
    type Error = &'static str;
    fn try_from(ipld: Ipld) -> Result<Self, Self::Error> {
        match ipld {
            Ipld::Map(node) => Ok(Self(node)),
            _ => Err("not a node"),
        }
    }
}

impl Deref for Node {
    type Target = BTreeMap<String, Ipld>;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl Node {
    // Write a link to the node. Use this for creating 'directories'
    pub fn put_link(&mut self, name: &str, link: &Cid) {
        assert_ne!(name, METADATA_KEY);
        self.0.insert(name.to_string(), Ipld::Link(link.clone()));
    }

    // Write a link to the node as an object. Use this for creating 'files'
    pub fn put_object_link(
        &mut self,
        name: &str,
        link: &Cid,
        maybe_metadata: Option<&BTreeMap<String, Ipld>>,
    ) {
        assert_ne!(name, METADATA_KEY);
        // Determine if we already have a metadata object for this link
        let metadata_ipld = self.0.get(METADATA_KEY).unwrap().clone();
        let mut metadata_map = match metadata_ipld {
            Ipld::Map(metadata) => metadata,
            _ => panic!("not a map"),
        };
        let object: Object = match metadata_map.get(name) {
            Some(object_ipld) => {
                let mut object = Object::try_from(object_ipld.clone()).unwrap();
                object.update(maybe_metadata);
                object
            }
            _ => Object::new(maybe_metadata),
        };
        metadata_map.insert(name.to_string(), object.into());
        self.0
            .insert(METADATA_KEY.to_string(), Ipld::Map(metadata_map.clone()));
        self.0.insert(name.to_string(), Ipld::Link(link.clone()));
    }

    // Remove a link from the node. Should return the CID of the link, as well as the fully
    // constructed object that was attached to the link if it exists (in the case of a file).
    pub fn del(&mut self, name: &str) -> (Cid, Option<Object>) {
        let metadata_ipld = self.0.get(METADATA_KEY).unwrap().clone();
        let mut metadata_map = match metadata_ipld {
            Ipld::Map(metadata) => metadata,
            _ => panic!("not a map"),
        };
        let link = self.0.remove(name).unwrap();
        let object = metadata_map.remove(name).unwrap();
        self.0
            .insert(METADATA_KEY.to_string(), Ipld::Map(metadata_map.clone()));
        match (link, object) {
            (Ipld::Link(cid), Ipld::Map(metadata)) => {
                let object = Object::try_from(Ipld::Map(metadata)).unwrap();
                (cid, Some(object))
            }
            (Ipld::Link(cid), _) => (cid, None),
            _ => panic!("not a link and metadata"),
        }
    }

    // Just get the link from the node, without any metadata
    pub fn get_link(&self, name: &str) -> Option<Cid> {
        assert_ne!(name, METADATA_KEY);
        self.0.get(name).and_then(|ipld| match ipld {
            Ipld::Link(cid) => Some(cid.clone()),
            _ => None,
        })
    }

    pub fn get_links(&self) -> BTreeMap<String, Cid> {
        let mut m = BTreeMap::new();
        for (k, v) in self.0.iter() {
            if k == METADATA_KEY {
                continue;
            }
            if let Ipld::Link(cid) = v {
                m.insert(k.clone(), cid.clone());
            }
        }
        m
    }

    // Get the fully constructed object from the node, if it exists
    pub fn get_object_metadata(&self, name: &str) -> Option<Object> {
        let metadata_ipld = self.0.get(METADATA_KEY).unwrap();
        let metadata_map = match metadata_ipld {
            Ipld::Map(metadata) => metadata,
            _ => panic!("not a map"),
        };
        metadata_map.get(name).map(|object_ipld| {
            let object = Object::try_from(object_ipld.clone()).unwrap();
            object
        })
    }

    // Get all the metadata objects from the node
    pub fn get_object_metadatas(&self) -> BTreeMap<String, Object> {
        let metadata_ipld = self.0.get(METADATA_KEY).unwrap();
        let metadata_map = match metadata_ipld {
            Ipld::Map(metadata) => metadata,
            _ => panic!("not a map"),
        };
        let mut m = BTreeMap::new();
        for (k, v) in metadata_map.iter() {
            let object = Object::try_from(v.clone()).unwrap();
            m.insert(k.clone(), object);
        }
        m
    }
}
