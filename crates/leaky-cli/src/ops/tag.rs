// use std::collections::BTreeMap;
// use std::path::PathBuf;

// use anyhow::anyhow;
// use leaky_common::prelude::*;
// use serde_json::Value;

// use super::change_log::ChangeType;
// use super::utils;

// fn clean_path(path: &PathBuf) -> PathBuf {
//     // Strip the / prefix
//     path.strip_prefix("/").unwrap().to_path_buf()
// }

// fn value_to_metadata(value: String) -> Result<BTreeMap<String, Ipld>, TagError> {
//     let mut metadata = BTreeMap::new();
//     let value: Value = serde_json::from_str(&value)?;
//     for (key, value) in value.as_object().unwrap() {
//         let ipld = match value {
//             Value::String(s) => Ipld::String(s.clone()),
//             Value::Number(n) => {
//                 if n.is_i64() {
//                     // Read as i128
//                     let i = n.as_i64().unwrap();
//                     Ipld::Integer(i as i128)
//                 } else {
//                     Ipld::Float(n.as_f64().unwrap())
//                 }
//             }
//             Value::Bool(b) => Ipld::Bool(*b),
//             Value::Null => Ipld::Null,
//             _ => return Err(TagError::Default(anyhow!("unsupported type: {:?}", value))),
//         };
//         metadata.insert(key.clone(), ipld);
//     }
//     Ok(metadata)
// }

// pub async fn tag(path: PathBuf, value: String) -> Result<Cid, TagError> {
//     let (mut leaky, change_log) = utils::load_on_disk().await?;
//     let mut updates = change_log.clone();

//     let root_cid = leaky.cid()?;
//     let metadata = value_to_metadata(value)?;
//     leaky.tag(&path, &metadata).await?;
//     let new_root_cid = leaky.cid()?;

//     if new_root_cid == root_cid {
//         println!("No changes to tag");
//         return Ok(root_cid);
//     }

//     // Get the path stripped of the / prefix
//     let path = clean_path(&path);
//     for (c_path, (cid, change)) in change_log.iter() {
//         if path == *c_path {
//             match change {
//                 ChangeType::Added { .. } => {
//                     updates.insert(c_path.clone(), (*cid, ChangeType::Added { modified: true }));
//                 }
//                 ChangeType::Base => {
//                     updates.insert(c_path.clone(), (*cid, ChangeType::Modified));
//                 }
//                 _ => {}
//             }
//         }
//     }

//     utils::save_on_disk(&mut leaky, &updates).await?;

//     Ok(new_root_cid)
// }

// #[derive(Debug, thiserror::Error)]
// pub enum TagError {
//     #[error("default error: {0}")]
//     Default(#[from] anyhow::Error),
//     #[error("cid error: {0}")]
//     Cid(#[from] libipld::cid::Error),
//     #[error("encountered mismatched cid: {0} != {1}")]
//     CidMismatch(Cid, Cid),
//     #[error("fs-tree error: {0}")]
//     FsTree(#[from] fs_tree::Error),
//     #[error("io error: {0}")]
//     Io(#[from] std::io::Error),
//     #[error("could not parse diff: {0}")]
//     Serde(#[from] serde_json::Error),
//     #[error("could not strip prefix: {0}")]
//     PathPrefix(#[from] std::path::StripPrefixError),
//     #[error("device error: {0}")]
//     Leaky(#[from] LeakyError),
// }
