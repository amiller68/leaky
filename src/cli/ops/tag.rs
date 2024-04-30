use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::cli::device::DeviceError;
use crate::types::{Audio, Schema, SchemaError, Visual, Writing};

use crate::cli::config::{Config, ConfigError};

// TODO: this whole tagging system is dissapointing. Look at all that bloat!
//  I either need to rethink this, or learn more about macros

fn input_tag<S>(path: &Path, value: &str) -> Result<Value, TagError>
where
    S: Schema,
{
    let path = path.to_path_buf();
    let extension = match path.extension() {
        Some(ext) => ext,
        None => return Err(TagError::NoExtension),
    };
    let extension_str = match extension.to_str() {
        Some(ext) => ext,
        None => return Err(TagError::NoExtension),
    };
    if !S::valid_extensions().contains(&extension_str) {
        return Err(TagError::UnsupportedFileType);
    }
    let value: Value = serde_json::from_str(value)?;
    let fields = S::fields();
    for (field, description) in fields {
        println!("{} | {}", field, description);
    }

    let schema = S::try_from(value.clone()).map_err(|_| TagError::Conversion)?;
    // Write the object as a schematized value
    Ok(schema.into_schema_value())
}

pub async fn tag(config: &Config, name: &str, path: &PathBuf, value: &str) -> Result<(), TagError> {
    // load the manifest schema
    let device = config.device()?;
    let mut change_log = config.change_log()?;
    let (_cid, base_manifest) = change_log.last_version().unwrap();
    let mut manifest = base_manifest.clone();
    let object = match manifest.get_object_mut(path) {
        Some(o) => o,
        None => return Err(TagError::ObjectDoesNotExist(path.clone())),
    };

    let value = match name {
        "writing" => input_tag::<Writing>(path, value),
        "audio" => input_tag::<Audio>(path, value),
        "visual" => input_tag::<Visual>(path, value),
        val => return Err(TagError::SchemaDoesNotExist(val.to_string())),
    }?;

    object.set_metdata(value);

    if base_manifest != &manifest {
        let cid = device.hash_manifest(&manifest, false).await?;
        let wtf_log = change_log.clone();
        let log = wtf_log.log();
        change_log.update(log, &manifest, &cid);

        config.set_change_log(change_log)?;
    }
    Ok(())
}

#[derive(Debug, thiserror::Error)]
pub enum TagError {
    #[error("config error: {0}")]
    Config(#[from] ConfigError),
    #[error("schema does not exist: {0}")]
    SchemaDoesNotExist(String),
    #[error("serde error: {0}")]
    Serde(#[from] serde_json::Error),
    #[error("error encoding schema")]
    Schema(#[from] SchemaError),
    #[error("device error: {0}")]
    Device(#[from] DeviceError),
    #[error("unsupported file type")]
    UnsupportedFileType,
    #[error("object does not exist: {0}")]
    ObjectDoesNotExist(PathBuf),
    #[error("conversion from value")]
    Conversion,
    #[error("file does not have an extension")]
    NoExtension,
}
