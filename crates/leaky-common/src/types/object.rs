use std::collections::BTreeMap;
use std::convert::TryFrom;

use chrono::Datelike;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use super::Ipld;

#[derive(Debug, PartialEq, Clone, Serialize, Deserialize)]
pub struct Object {
    created_at: OffsetDateTime,
    updated_at: OffsetDateTime,
    metadata: BTreeMap<String, Ipld>,
}

impl Default for Object {
    fn default() -> Self {
        Object {
            created_at: OffsetDateTime::now_utc(),
            updated_at: OffsetDateTime::now_utc(),
            metadata: BTreeMap::new(),
        }
    }
}

const OBJECT_CREATED_AT_LABEL: &str = "created_at";
const OBJECT_UPDATED_AT_LABEL: &str = "updated_at";
const OBJECT_METADATA_LABEL: &str = "metadata";

impl From<Object> for Ipld {
    fn from(object: Object) -> Self {
        let mut map = BTreeMap::new();
        map.insert(
            OBJECT_CREATED_AT_LABEL.to_string(),
            Ipld::Integer(object.created_at().unix_timestamp_nanos()),
        );
        map.insert(
            OBJECT_UPDATED_AT_LABEL.to_string(),
            Ipld::Integer(object.updated_at().unix_timestamp_nanos()),
        );
        map.insert(
            OBJECT_METADATA_LABEL.to_string(),
            Ipld::Map(object.metadata().clone()),
        );
        Ipld::Map(map)
    }
}

impl TryFrom<Ipld> for Object {
    type Error = ObjectIpldError;
    fn try_from(ipld: Ipld) -> Result<Self, ObjectIpldError> {
        let map = match ipld {
            Ipld::Map(map) => map,
            _ => return Err(ObjectIpldError::NotMap),
        };

        let created_at_int = match map.get(OBJECT_CREATED_AT_LABEL) {
            Some(Ipld::Integer(created_at)) => *created_at,
            _ => {
                return Err(ObjectIpldError::MissingMapMember(
                    OBJECT_CREATED_AT_LABEL.to_string(),
                ))
            }
        };
        let created_at = OffsetDateTime::from_unix_timestamp_nanos(created_at_int)?;

        let updated_at_int = match map.get(OBJECT_UPDATED_AT_LABEL) {
            Some(Ipld::Integer(updated_at)) => *updated_at,
            _ => {
                return Err(ObjectIpldError::MissingMapMember(
                    OBJECT_UPDATED_AT_LABEL.to_string(),
                ))
            }
        };
        let updated_at = OffsetDateTime::from_unix_timestamp_nanos(updated_at_int)?;

        let metadata = match map.get(OBJECT_METADATA_LABEL) {
            Some(Ipld::Map(metadata)) => metadata.clone(),
            _ => {
                return Err(ObjectIpldError::MissingMapMember(
                    OBJECT_METADATA_LABEL.to_string(),
                ))
            }
        };

        Ok(Self {
            created_at,
            updated_at,
            metadata,
        })
    }
}

impl Object {
    pub fn new(maybe_metadata: Option<&BTreeMap<String, Ipld>>) -> Self {
        let metadata = match maybe_metadata {
            Some(metadata) => metadata.clone(),
            None => BTreeMap::new(),
        };
        Object {
            created_at: OffsetDateTime::now_utc(),
            updated_at: OffsetDateTime::now_utc(),
            metadata,
        }
    }

    /* Getters */

    pub fn created_at(&self) -> &OffsetDateTime {
        &self.created_at
    }

    pub fn updated_at(&self) -> &OffsetDateTime {
        &self.updated_at
    }

    pub fn metadata(&self) -> &BTreeMap<String, Ipld> {
        &self.metadata
    }

    /* Updaters */

    /// Update the data, metadata or both
    pub fn update(
        &mut self,
        maybe_metadata: Option<&BTreeMap<String, Ipld>>,
        maybe_backdate: Option<chrono::NaiveDate>,
    ) {
        if let Some(naive_date) = maybe_backdate {
            let time_date = time::Date::from_calendar_date(
                naive_date.year(),
                time::Month::try_from(naive_date.month() as u8).unwrap(),
                naive_date.day() as u8,
            )
            .unwrap();

            // Create a time::OffsetDateTime from the date, assuming midnight UTC
            let offset_dt = time_date.with_hms(0, 0, 0).unwrap().assume_utc();

            self.created_at = offset_dt;
        }
        self.updated_at = OffsetDateTime::now_utc();
        if let Some(metadata) = maybe_metadata {
            self.metadata = metadata.clone();
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ObjectIpldError {
    #[error("invalid datetime: {0}")]
    InvalidDateTime(#[from] time::error::ComponentRange),
    #[error("missing map member: {0}")]
    MissingMapMember(String),
    #[error("serde json error: {0}")]
    SerdeJson(#[from] serde_json::Error),
    #[error("ipld data is not map")]
    NotMap,
}
