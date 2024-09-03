use axum::extract::{Path, State};
use axum::response::{IntoResponse, Response};
use axum_extra::headers::ContentType;
use axum_extra::TypedHeader;
use axum::http::header::CONTENT_TYPE;
use std::path::PathBuf;

use leaky_common::prelude::*;

use crate::app::AppState;
use crate::database::models::RootCid;

pub async fn handler(
    State(state): State<AppState>,
    Path(path): Path<PathBuf>,
    // TypedHeader(content_type): TypedHeader<ContentType>,
) -> Result<impl IntoResponse, GetContentError> {
    // TODO: match on content_type to determine the response

    let db = state.sqlite_database();
    let mut conn = db.acquire().await?;
    let maybe_root_cid = RootCid::pull(&mut conn).await?;
    let root_cid = match maybe_root_cid {
        Some(rc) => rc,
        None => return Err(GetContentError::RootNotFound),
    };

    let ipfs_rpc = state.ipfs_rpc();
    let mount = Mount::pull(root_cid.cid(), &ipfs_rpc).await?;

    // Make the path absolute
    let path = PathBuf::from("/").join(path);
    let data_result = mount.cat(&path).await;
    tracing::info!("GET /{}: {:?}", path.display(), data_result);
    match data_result {
        Ok(data) => {
            // TODO: i should check what the extension is and set the content type accordingly
            //  For now, just return everything as text/plain
            return Ok((http::StatusCode::OK, 
                [(CONTENT_TYPE, "text/plain")],
                data).into_response());
        }
        Err(MountError::PathNotFile(_)) => {},
        Err(e) => return Err(GetContentError::Mount(e)),
    }

    let ls_result = mount.ls(&path).await;
    let ls = match ls_result {
        Ok(ls) => ls,
        Err(MountError::PathNotDir(_)) => return Err(GetContentError::NotFound),
        Err(e) => return Err(GetContentError::Mount(e)),
    };
    let ls_json = serde_json::to_string(&ls).unwrap();

    Ok((http::StatusCode::OK, ls_json).into_response())
}

#[derive(Debug, thiserror::Error)]
pub enum GetContentError {
    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),
    #[error("root CID error: {0}")]
    RootCid(#[from] crate::database::models::RootCidError),
    #[error("No root CID found")]
    RootNotFound,
    #[error("not found")]
    NotFound,
    #[error("mount error: {0}")]
    Mount(#[from] MountError),

}

impl IntoResponse for GetContentError {
    fn into_response(self) -> Response {
        match self {
            GetContentError::Mount(_) |
            GetContentError::RootCid(_) |
            GetContentError::Database(_) => {
                tracing::error!("{:?}", self);
                return (
                http::StatusCode::INTERNAL_SERVER_ERROR,
                "unknown server error",
            )
                .into_response()},
            GetContentError::RootNotFound => {
                (http::StatusCode::NOT_FOUND, "No root CID found").into_response()
            }
            GetContentError::NotFound => {
                (http::StatusCode::NOT_FOUND, "Not found").into_response()
            }
        }
    }
}
