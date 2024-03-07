use std::env;

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct Version {
    pub build_profile: String,
    pub build_features: String,
    pub repo_version: String,
    pub version: String,
}

impl Default for Version {
    fn default() -> Self {
        Self::new()
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
}
