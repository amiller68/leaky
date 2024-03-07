use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::version::Version;

use crate::backend::Cid;

// Manifest
pub struct Manifest {
    version: Version,
    previosus: Cid,
}
