use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::version::Version;

use crate::backend::Cid;

// Manifest
#[derive(Serialize, Deserialize, Debug)]
pub struct Manifest {
    version: Version,
    previosus: Cid,
}

impl Manifest {
    pub fn new() -> Self {
        let version = Version::new();
        let previosus = Cid::default();
        Self { version, previosus }
    }
}
