use std::collections::BTreeMap;
use std::convert::TryFrom;
use std::env;

use serde::{Deserialize, Serialize};

use super::Ipld;

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct Version {
    build_profile: String,
    build_features: String,
    repo_version: String,
    version: String,
}

impl Default for Version {
    fn default() -> Self {
        Self::new()
    }
}

impl Into<Ipld> for Version {
    fn into(self) -> Ipld {
        let mut map = BTreeMap::new();
        map.insert(
            "build_profile".to_string(),
            Ipld::String(self.build_profile.to_string()),
        );
        map.insert(
            "build_features".to_string(),
            Ipld::String(self.build_features.to_string()),
        );
        map.insert(
            "repo_version".to_string(),
            Ipld::String(self.repo_version.to_string()),
        );
        map.insert(
            "version".to_string(),
            Ipld::String(self.version.to_string()),
        );
        Ipld::Map(map)
    }
}

impl TryFrom<Ipld> for Version {
    type Error = VersionError;
    fn try_from(ipld: Ipld) -> Result<Self, VersionError> {
        match ipld {
            Ipld::Map(map) => {
                let version = match map.get("version") {
                    Some(Ipld::String(version)) => version.to_string(),
                    _ => return Err(VersionError::MissingMetadata("version".to_string())),
                };
                let build_profile = match map.get("build_profile") {
                    Some(Ipld::String(build_profile)) => build_profile.to_string(),
                    _ => return Err(VersionError::MissingMetadata("build_profile".to_string())),
                };
                let build_features = match map.get("build_features") {
                    Some(Ipld::String(build_features)) => build_features.to_string(),
                    _ => return Err(VersionError::MissingMetadata("build_features".to_string())),
                };
                let repo_version = match map.get("repo_version") {
                    Some(Ipld::String(repo_version)) => repo_version.to_string(),
                    _ => return Err(VersionError::MissingMetadata("repo_version".to_string())),
                };

                Ok(Self {
                    build_profile,
                    build_features,
                    repo_version,
                    version,
                })
            }
            _ => Err(VersionError::MissingMetadata("map".to_string())),
        }
    }
}

impl Version {
    pub fn new() -> Self {
        Self {
            build_profile: env!("BUILD_PROFILE").to_string(),
            build_features: env!("BUILD_FEATURES").to_string(),
            repo_version: env!("REPO_VERSION").to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
        }
    }

    pub fn build_profile(&self) -> &str {
        &self.build_profile
    }

    pub fn build_features(&self) -> &str {
        &self.build_features
    }

    pub fn repo_version(&self) -> &str {
        &self.repo_version
    }

    pub fn version(&self) -> &str {
        &self.version
    }
}

#[derive(Debug, thiserror::Error)]
pub enum VersionError {
    #[error("missing metadata: {0}")]
    MissingMetadata(String),
}
