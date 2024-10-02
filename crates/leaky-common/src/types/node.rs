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
        self.0.insert(name.to_string(), Ipld::Link(*link));
    }

    pub fn put_object(&mut self, name: &str, maybe_metadata: Option<&BTreeMap<String, Ipld>>, maybe_backdate: Option<chrono::NaiveDate>) {
        assert_ne!(name, METADATA_KEY);
        let metadata_ipld = self.0.get(METADATA_KEY).unwrap().clone();
        let mut metadata_map = match metadata_ipld {
            Ipld::Map(metadata) => metadata,
            _ => panic!("not a map"),
        };
        let object: Object = match metadata_map.get(name) {
            Some(object_ipld) => {
                let mut object = Object::try_from(object_ipld.clone()).unwrap();
                object.update(maybe_metadata, maybe_backdate);
                object
            }
            _ => Object::new(maybe_metadata),
        };
        metadata_map.insert(name.to_string(), object.into());
        self.0
            .insert(METADATA_KEY.to_string(), Ipld::Map(metadata_map.clone()));
    }

    // Write a link to the node as an object. Use this for creating 'files'
    pub fn update_link(
        &mut self,
        name: &str,
        maybe_link: Option<&Cid>,
        maybe_metadata: Option<&BTreeMap<String, Ipld>>,
        maybe_backdate: Option<chrono::NaiveDate>,
    ) {
        assert_ne!(name, METADATA_KEY);

        if let Some(link) = maybe_link {
            self.put_link(name, link);
        }
        self.put_object(name, maybe_metadata, maybe_backdate)
    }

    // Remove a link from the node. Should return the CID of the link, as well as the fully
    // constructed object that was attached to the link if it exists (in the case of a file).
    pub fn del(&mut self, name: &str) -> (Option<Cid>, Option<Object>) {
        let metadata_ipld = self.0.get(METADATA_KEY).unwrap().clone();
        let mut metadata_map = match metadata_ipld {
            Ipld::Map(metadata) => metadata,
            _ => panic!("not a map"),
        };
        // Match on whether the link is present in the node
        let link = match self.0.remove(name) {
            Some(Ipld::Link(cid)) => Ipld::Link(cid),
            None => return (Some(Cid::default()), None),
            _ => panic!("not a link"),
        };
        let object = metadata_map.remove(name);
        self.0
            .insert(METADATA_KEY.to_string(), Ipld::Map(metadata_map.clone()));
        match (link, object) {
            (Ipld::Link(cid), Some(Ipld::Map(metadata))) => {
                let object = Object::try_from(Ipld::Map(metadata)).unwrap();
                (Some(cid), Some(object))
            }
            (Ipld::Link(cid), None) => (Some(cid), None),
            _ => panic!("not a link and metadata"),
        }
    }

    // Just get the link from the node, without any metadata
    pub fn get_link(&self, name: &str) -> Option<Cid> {
        assert_ne!(name, METADATA_KEY);
        self.0.get(name).and_then(|ipld| match ipld {
            Ipld::Link(cid) => Some(*cid),
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
                m.insert(k.clone(), *cid);
            }
        }
        m
    }

    pub fn size(&self) -> usize {
        // Get the length of the node, minus the metadata key
        self.0.len() - 1
    }

    // Get the fully constructed object from the node, if it exists
    pub fn get_object(&self, name: &str) -> Option<Object> {
        let metadata_ipld = self.0.get(METADATA_KEY).unwrap();
        let metadata_map = match metadata_ipld {
            Ipld::Map(metadata) => metadata,
            _ => panic!("not a map"),
        };
        metadata_map
            .get(name)
            .map(|object_ipld| Object::try_from(object_ipld.clone()).unwrap())
    }

    // Get all the metadata objects from the node
    pub fn get_objects(&self) -> BTreeMap<String, Object> {
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
