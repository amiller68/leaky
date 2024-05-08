use std::convert::TryFrom;

use serde::{Deserialize, Serialize};

use super::version::Version;
use super::{Cid, Ipld};

/// Manifest
#[derive(Default, Debug, PartialEq, Clone, Serialize, Deserialize)]
pub struct Manifest {
    /// Build version
    version: Version,
    /// Previous manifest CID
    previous: Cid,
    /// Root node CID
    root: Cid,
}

impl Into<Ipld> for Manifest {
    fn into(self) -> Ipld {
        let mut map = std::collections::BTreeMap::new();
        map.insert("version".to_string(), self.version.clone().into());
        map.insert("previous".to_string(), Ipld::Link(self.previous().clone()));
        map.insert("root".to_string(), Ipld::Link(self.root.clone()));
        Ipld::Map(map)
    }
}

impl TryFrom<Ipld> for Manifest {
    type Error = ManifestError;
    fn try_from(ipld: Ipld) -> Result<Self, ManifestError> {
        match ipld {
            Ipld::Map(map) => {
                let version = match map.get("version") {
                    Some(ipld) => Version::try_from(ipld.clone())?,
                    None => return Err(ManifestError::MissingField("version".to_string())),
                };
                let previous = match map.get("previous") {
                    Some(Ipld::Link(cid)) => *cid,
                    _ => return Err(ManifestError::MissingField("previous link".to_string())),
                };
                let root = match map.get("root") {
                    Some(Ipld::Link(cid)) => *cid,
                    _ => return Err(ManifestError::MissingField("root link".to_string())),
                };

                Ok(Manifest {
                    version,
                    previous,
                    root,
                })
            }
            _ => Err(ManifestError::MissingField("map".to_string())),
        }
    }
}

impl Manifest {
    pub fn version(&self) -> &Version {
        &self.version
    }

    pub fn previous(&self) -> &Cid {
        &self.previous
    }

    pub fn root(&self) -> &Cid {
        &self.root
    }

    pub fn set_root(&mut self, cid: Cid) {
        self.root = cid;
    }

    pub fn set_previous(&mut self, cid: Cid) {
        self.previous = cid;
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ManifestError {
    #[error("version error")]
    VersionError(#[from] super::version::VersionError),
    #[error("missing field: {0}")]
    MissingField(String),
}
