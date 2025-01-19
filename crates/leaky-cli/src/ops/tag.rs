use std::collections::BTreeMap;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value;

use leaky_common::prelude::*;

use crate::change_log::ChangeType;
use crate::{AppState, Op};

#[derive(Debug, clap::Args, Clone)]
pub struct Tag {
    #[clap(short, long)]
    path: PathBuf,
    #[clap(short, long)]
    value: String,
    #[clap(short, long)]
    backdate: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum TagError {
    #[error("app state error: {0}")]
    AppState(#[from] crate::state::AppStateSetupError),
    #[error("api error: {0}")]
    Api(#[from] leaky_common::error::ApiError),
    #[error("unsupported value type")]
    UnsupportedValueType,
    #[error("cid error: {0}")]
    Cid(#[from] leaky_common::error::CidError),
    #[error("could not parse diff: {0}")]
    Serde(#[from] serde_json::Error),
    #[error("could not strip prefix: {0}")]
    PathPrefix(#[from] std::path::StripPrefixError),
    #[error("mount error: {0}")]
    Mount(#[from] MountError),
    #[error("invalid backdate: {0}")]
    InvalidBackdate(#[from] chrono::ParseError),
}

#[async_trait]
impl Op for Tag {
    type Error = TagError;
    type Output = Cid;

    async fn execute(&self, state: &AppState) -> Result<Self::Output, Self::Error> {
        let cid = *state.cid();
        let change_log = state.change_log().clone();
        let mut updates = change_log.clone();
        let ipfs_rpc = Arc::new(state.client()?.ipfs_rpc()?);
        let mut mount = Mount::pull(cid, &ipfs_rpc).await?;

        let path = self.path.clone();
        let value = self.value.clone();
        let backdate = match &self.backdate {
            Some(bd) => Some(chrono::NaiveDate::parse_from_str(bd, "%Y-%m-%d")?),
            None => None,
        };

        let metadata = value_to_metadata(value)?;
        mount.tag(&path, &metadata, backdate).await?;
        mount.push().await?;
        let new_cid = mount.cid();

        if *new_cid == cid {
            println!("No changes to tag");
            return Ok(cid);
        }

        // Get the path stripped of the / prefix
        let path = clean_path(&path);
        for (c_path, (cid, change)) in change_log.iter() {
            if path == *c_path && change == &ChangeType::Base {
                updates.insert(c_path.clone(), (*cid, ChangeType::Modified));
            }
        }

        state.save(&mount, Some(&updates), None)?;

        Ok(cid)
    }
}

fn clean_path(path: &Path) -> PathBuf {
    // Strip the / prefix
    path.strip_prefix("/").unwrap().to_path_buf()
}

fn value_to_metadata(value: String) -> Result<BTreeMap<String, Ipld>, TagError> {
    let mut metadata = BTreeMap::new();
    let value: Value = serde_json::from_str(&value)?;
    for (key, value) in value.as_object().unwrap() {
        let ipld = match value {
            Value::String(s) => Ipld::String(s.clone()),
            Value::Number(n) => {
                if n.is_i64() {
                    // Read as i128
                    let i = n.as_i64().unwrap();
                    Ipld::Integer(i as i128)
                } else {
                    Ipld::Float(n.as_f64().unwrap())
                }
            }
            Value::Bool(b) => Ipld::Bool(*b),
            Value::Null => Ipld::Null,
            _ => return Err(TagError::UnsupportedValueType),
        };
        metadata.insert(key.clone(), ipld);
    }
    Ok(metadata)
}
