use serde::{Deserialize, Serialize};

use super::version::Version;
use super::{Cid, Ipld};

use crate::traits::Blockable;

// Manifest
#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
pub struct Manifest {
    version: Version,
    previosus: Cid,
}

impl Blockable for Manifest {
    type Error = ManifestError;
    fn to_ipld(&self) -> libipld::Ipld {
        let mut map = std::collections::BTreeMap::new();
        map.insert("version".to_string(), self.version.to_ipld());
        map.insert("previosus".to_string(), Ipld::Link(self.previous().clone()));
        Ipld::Map(map)
    }
    fn from_ipld(ipld: &libipld::Ipld) -> Result<Self, Self::Error> {
        match ipld {
            Ipld::Map(map) => {
                let version = match map.get("version") {
                    Some(ipld) => Version::from_ipld(ipld)?,
                    None => return Err(ManifestError::MissingField("version".to_string())),
                };
                let previosus = match map.get("previosus") {
                    Some(Ipld::Link(cid)) => cid.clone(),
                    _ => return Err(ManifestError::MissingField("previosus".to_string())),
                };
                Ok(Manifest { version, previosus })
            }
            _ => Err(ManifestError::MissingField("metadata".to_string())),
        }
    }
}

impl Manifest {
    pub fn new() -> Self {
        let version = Version::new();
        let previosus = Cid::default();
        Self { version, previosus }
    }

    pub fn version(&self) -> &Version {
        &self.version
    }

    pub fn previous(&self) -> &Cid {
        &self.previosus
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ManifestError {
    #[error("version error")]
    VersionError(#[from] super::version::VersionError),
    #[error("missing field: {0}")]
    MissingField(String),
}
