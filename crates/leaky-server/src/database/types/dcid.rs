use multibase::Base;
use serde::{Deserialize, Serialize};
use sqlx::encode::IsNull;
use sqlx::error::BoxDynError;
use sqlx::sqlite::{SqliteArgumentValue, SqliteTypeInfo, SqliteValueRef};
use sqlx::{Decode, Encode, Sqlite, Type};

use leaky_common::prelude::Cid;

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(transparent)]
pub struct DCid(Cid);

impl From<DCid> for Cid {
    fn from(val: DCid) -> Self {
        val.0
    }
}

impl From<Cid> for DCid {
    fn from(cid: Cid) -> Self {
        Self(cid)
    }
}

impl Decode<'_, Sqlite> for DCid {
    fn decode(value: SqliteValueRef<'_>) -> Result<Self, BoxDynError> {
        let db_val = <String as Decode<Sqlite>>::decode(value)?;
        let cid = Cid::try_from(db_val).map_err(DCidError::InvalidCid)?;

        Ok(Self(cid))
    }
}

impl Encode<'_, Sqlite> for DCid {
    fn encode_by_ref(&self, args: &mut Vec<SqliteArgumentValue<'_>>) -> IsNull {
        args.push(SqliteArgumentValue::Text(
            self.0.to_string_of_base(Base::Base32Lower).unwrap().into(),
        ));
        IsNull::No
    }
}

impl Type<Sqlite> for DCid {
    fn compatible(ty: &SqliteTypeInfo) -> bool {
        <String as Type<Sqlite>>::compatible(ty)
    }

    fn type_info() -> SqliteTypeInfo {
        <String as Type<Sqlite>>::type_info()
    }
}

#[derive(Debug, thiserror::Error)]
pub enum DCidError {
    #[error("invalid cid: {0}")]
    InvalidCid(#[from] leaky_common::error::CidError),
}
