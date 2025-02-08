use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Serialize, Deserialize)]
pub struct TestConfig {
    pub server_port: u16,
    pub fixtures_dir: PathBuf,
    pub scratch_dir: PathBuf,
}

impl Default for TestConfig {
    fn default() -> Self {
        Self {
            server_port: 3001,
            fixtures_dir: PathBuf::from("../../example"),
            scratch_dir: PathBuf::from("../../data/test"),
        }
    }
} 