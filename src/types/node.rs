use std::collections::BTreeMap;
use std::convert::TryFrom;
use std::ops::Deref;

use super::ipld::{Cid, Ipld};

#[derive(Debug, Default, Clone, PartialEq)]
pub struct Node(BTreeMap<String, Ipld>);

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
    pub fn add(&mut self, name: &str, link: &Cid) {
        self.0.insert(name.to_string(), Ipld::Link(link.clone()));
    }
    pub fn remove(&mut self, name: &str) {
        self.0.remove(name);
    }
    pub fn get(&self, name: &str) -> Option<&Cid> {
        self.0.get(name).and_then(|ipld| match ipld {
            Ipld::Link(cid) => Some(cid),
            _ => None,
        })
    }
}
