use std::convert::TryFrom;

use serde::{Deserialize, Serialize};

use super::version::Version;
use super::{Cid, Ipld};

// Manifest
#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
pub struct Manifest {
    version: Version,
    previosus: Cid,
}

impl Into<Ipld> for Manifest {
    fn into(self) -> Ipld {
        let mut map = std::collections::BTreeMap::new();
        map.insert("version".to_string(), self.version.clone().into());
        map.insert("previosus".to_string(), Ipld::Link(self.previous().clone()));
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
