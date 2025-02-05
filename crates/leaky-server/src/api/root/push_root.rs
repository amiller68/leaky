use std::str::FromStr;

use axum::extract::{Json, State};
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};

use leaky_common::prelude::Cid;

use crate::app::AppState;
use crate::database::models::RootCid;

#[derive(Deserialize)]
pub struct PushRootRequest {
    cid: String,
    previous_cid: String,
}

#[derive(Serialize)]
pub struct PushRootResponse {
    previous_cid: String,
    cid: String,
}

impl From<RootCid> for PushRootResponse {
    fn from(root_cid: RootCid) -> Self {
        PushRootResponse {
            previous_cid: root_cid.previous_cid().to_string(),
            cid: root_cid.cid().to_string(),
        }
    }
}

pub async fn handler(
    State(state): State<AppState>,
    Json(push_root): Json<PushRootRequest>,
) -> Result<impl IntoResponse, PushRootError> {
    let cid = Cid::from_str(&push_root.cid)?;
    let previous_cid = Cid::from_str(&push_root.previous_cid)?;
    let mut mount = state.mount();

    let db = state.sqlite_database();
    let mut conn = db.begin().await?;

    let root_cid = RootCid::push(&cid, &previous_cid, &mut conn).await?;

    conn.commit().await?;

    // TODO: if this fails this could never retry properly and mess up versioning
    //  This shoudl really be backgrounded in order to be considered correct
    // TODO: i am not sure if old blocks get pruged from the metadata on pull ...
    //  this not being the case has the potential to cause bloat
    mount.update(root_cid.cid()).await?;
    tracing::info!("mounted new root CID: {}", root_cid.cid());
    tracing::info!("previous root CID: {}", root_cid.previous_cid());
    tracing::info!("updated manifest: {:?}", mount.manifest());

    Ok((http::StatusCode::OK, Json(PushRootResponse::from(root_cid))).into_response())
}

#[derive(Debug, thiserror::Error)]
pub enum PushRootError {
    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),
    #[error("invalid CID: {0}")]
    Cid(#[from] leaky_common::error::CidError),
    #[error("root CID error: {0}")]
    RootCid(#[from] crate::database::models::RootCidError),
    #[error("mount error: {0}")]
    MountError(#[from] leaky_common::error::MountError),
}

impl IntoResponse for PushRootError {
    fn into_response(self) -> Response {
        match self {
            PushRootError::MountError(_) | PushRootError::Database(_) => (
                http::StatusCode::INTERNAL_SERVER_ERROR,
                "unknown server error",
            )
                .into_response(),
            PushRootError::Cid(_err) => {
                (http::StatusCode::BAD_REQUEST, "invalid cid").into_response()
            }
            PushRootError::RootCid(ref err) => match err {
                crate::database::models::RootCidError::Sqlx(err) => {
                    tracing::error!("database error: {}", err);
                    (
                        http::StatusCode::INTERNAL_SERVER_ERROR,
                        "unknown server error",
                    )
                        .into_response()
                }
                crate::database::models::RootCidError::InvalidLink(_, _) => {
                    (http::StatusCode::BAD_REQUEST, "invalid link").into_response()
                }
                crate::database::models::RootCidError::Conflict(_, _) => {
                    (http::StatusCode::CONFLICT, "conflict").into_response()
                }
            },
        }
    }
}
