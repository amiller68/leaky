use std::str::FromStr;

use axum::extract::{Json, State};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::Router;
use cid::Cid;
use http::header::{ACCEPT, ORIGIN};
use http::Method;
use serde::{Deserialize, Serialize};
use tower_http::cors::{Any, CorsLayer};

use crate::app::AppState;
use crate::database::models::RootCid;

pub fn router(state: AppState) -> Router<AppState> {
    let cors_layer = CorsLayer::new()
        .allow_methods(vec![Method::GET])
        .allow_headers(vec![ACCEPT, ORIGIN])
        .allow_origin(Any)
        .allow_credentials(false);

    Router::new()
        .route("/root", get(pull_root).post(push_root))
        .with_state(state)
        .layer(cors_layer)
}

#[derive(Serialize)]
pub struct PullRootResponse {
    previous_cid: String,
    cid: String,
}

impl From<RootCid> for PullRootResponse {
    fn from(root_cid: RootCid) -> Self {
        PullRootResponse {
            previous_cid: root_cid.previous_cid().to_string(),
            cid: root_cid.cid().to_string(),
        }
    }
}

pub async fn pull_root(State(state): State<AppState>) -> Result<impl IntoResponse, PullRootError> {
    let db = state.sqlite_database();
    let mut conn = db.acquire().await?;
    let maybe_root_cid = RootCid::pull(&mut conn).await?;
    match maybe_root_cid {
        Some(root_cid) => {
            Ok((http::StatusCode::OK, Json(PullRootResponse::from(root_cid))).into_response())
        }
        None => Err(PullRootError::NotFound),
    }
}

#[derive(Debug, thiserror::Error)]
pub enum PullRootError {
    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),
    #[error("root CID error: {0}")]
    RootCid(#[from] crate::database::models::RootCidError),
    #[error("No root CID found")]
    NotFound,
}

impl IntoResponse for PullRootError {
    fn into_response(self) -> Response {
        match self {
            PullRootError::Database(_) => (
                http::StatusCode::INTERNAL_SERVER_ERROR,
                "unknown server error",
            )
                .into_response(),
            PullRootError::RootCid(_) => (
                http::StatusCode::INTERNAL_SERVER_ERROR,
                "unknown server error",
            )
                .into_response(),
            PullRootError::NotFound => {
                (http::StatusCode::NOT_FOUND, "No root CID found").into_response()
            }
        }
    }
}

#[derive(Deserialize)]
pub struct PushRootRequest {
    cid: String,
    previous_cid: String,
}

pub async fn push_root(
    State(state): State<AppState>,
    Json(push_root): Json<PushRootRequest>,
) -> Result<impl IntoResponse, PushRootError> {
    let cid = Cid::from_str(&push_root.cid)?;
    let previous_cid = Cid::from_str(&push_root.previous_cid)?;

    let db = state.sqlite_database();
    let mut conn = db.begin().await?;

    let root_cid = RootCid::push(&cid, &previous_cid, &mut conn).await?;

    conn.commit().await?;

    Ok((http::StatusCode::OK, Json(PullRootResponse::from(root_cid))).into_response())
}

#[derive(Debug, thiserror::Error)]
pub enum PushRootError {
    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),
    #[error("invalid CID: {0}")]
    Cid(#[from] cid::Error),
    #[error("root CID error: {0}")]
    RootCid(#[from] crate::database::models::RootCidError),
}

impl IntoResponse for PushRootError {
    fn into_response(self) -> Response {
        match self {
            PushRootError::Database(err) => {
                tracing::error!("database error: {}", err);
                (
                    http::StatusCode::INTERNAL_SERVER_ERROR,
                    "unknown server error",
                )
                    .into_response()
            }
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
