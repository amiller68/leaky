use std::collections::BTreeMap;
use std::convert::TryFrom;
use std::path::PathBuf;

use super::object::Object;
use super::version::Version;
use super::{Cid, Ipld};

/// Manifest
#[derive(Default, Debug, PartialEq, Clone)]
pub struct Manifest {
    /// Build version
    version: Version,
    /// Previous manifest CID
    previosus: Cid,
    // NOTE: it would be good to research the extent to which you can customize gateway traversal
    // and response generation to allow for a more "native" experience
    /// Root CID of the store's content -- implemented as links to enable Gateway traversal
    root: Cid,
    /// Map to links to object blocks
    objects: BTreeMap<PathBuf, Cid>,
}

impl Into<Ipld> for Manifest {
    fn into(self) -> Ipld {
        let mut map = std::collections::BTreeMap::new();
        map.insert("version".to_string(), self.version.clone().into());
        map.insert("previosus".to_string(), Ipld::Link(self.previous().clone()));
        map.insert("root".to_string(), Ipld::Link(self.root.clone()));
        map.insert(
            "objects".to_string(),
            Ipld::Map(
                self.objects
                    .iter()
                    .map(|(k, v)| (k.to_string_lossy().to_string(), Ipld::Link(v.clone())))
                    .collect(),
            ),
        );
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
                    _ => return Err(ManifestError::MissingField("previosus link".to_string())),
                };
                let root = match map.get("root") {
                    Some(Ipld::Link(cid)) => cid.clone(),
                    _ => return Err(ManifestError::MissingField("root link".to_string())),
                };
                let objects = match map.get("objects") {
                    Some(Ipld::Map(objects)) => objects
                        .iter()
                        .map(|(k, v)| {
                            let path = PathBuf::from(k);
                            let cid = match v {
                                Ipld::Link(cid) => cid.clone(),
                                _ => {
                                    return Err(ManifestError::MissingField(
                                        "objects link".to_string(),
                                    ))
                                }
                            };
                            Ok((path, cid))
                        })
                        .collect::<Result<BTreeMap<PathBuf, Cid>, ManifestError>>()?,
                    _ => return Err(ManifestError::MissingField("objects map".to_string())),
                };

                Ok(Manifest {
                    version,
                    previosus,
                    root,
                    objects,
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
        &self.previosus
    }

    pub fn root(&self) -> &Cid {
        &self.root
    }

    pub fn objects(&self) -> &BTreeMap<PathBuf, Cid> {
        &self.objects
    }

    pub fn set_root(&mut self, cid: Cid) {
        self.root = cid;
    }

    pub fn link_object(&mut self, path: PathBuf, cid: Cid) {
        self.objects.insert(path, cid);
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ManifestError {
    #[error("version error")]
    VersionError(#[from] super::version::VersionError),
    #[error("missing field: {0}")]
    MissingField(String),
}
