use axum::extract::{Json, Path, Query, State};
use axum::http::header::CONTENT_TYPE;
use axum::response::{IntoResponse, Response};
use regex::Regex;
use std::path::PathBuf;
use url::Url;

use leaky_common::prelude::*;

use crate::app::AppState;
use crate::database::models::RootCid;

#[derive(Debug, serde::Deserialize)]
pub struct GetContentQuery {
    pub html: Option<bool>,
}

pub async fn handler(
    State(state): State<AppState>,
    Path(path): Path<PathBuf>,
    Query(query): Query<GetContentQuery>,
) -> Result<impl IntoResponse, GetContentError> {
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
    match data_result {
        Ok(data) => {
            // TODO: i should check what the extension is and set the content type accordingly
            //  For now, just return everything as text/plain
            let ext = path
                .extension()
                .unwrap_or_default()
                .to_str()
                .unwrap_or_default();
            match ext {
                // Markdown
                "md" => {
                    if query.html.unwrap_or(false) {
                        tracing::info!(
                            "GET {} | {:?} | rendering markdown as html",
                            path.display(),
                            query
                        );
                        let base_path = path.parent();
                        let empty_path = PathBuf::new();
                        let base_path = base_path.unwrap_or(&empty_path);
                        let html = markdown_to_html(
                            data,
                            &base_path.to_path_buf(),
                            &state.get_content_forwarding_url().join("content").unwrap(),
                        );

                        return Ok((http::StatusCode::OK, [(CONTENT_TYPE, "text/html")], html)
                            .into_response());
                    };
                    tracing::info!(
                        "GET {} | {:?} | returning markdown as text",
                        path.display(),
                        query
                    );
                    return Ok((http::StatusCode::OK, [(CONTENT_TYPE, "text/plain")], data)
                        .into_response());
                }
                // Images
                "png" | "jpg" | "jpeg" | "gif" => {
                    tracing::info!("GET {} | {:?} | returning image", path.display(), query);
                    return Ok(
                        (http::StatusCode::OK, [(CONTENT_TYPE, "image")], data).into_response()
                    );
                }
                // All other files
                _ => {
                    tracing::info!("GET {} | {:?} | returning misc file", path.display(), query);
                    return Ok((http::StatusCode::OK, [(CONTENT_TYPE, "text/plain")], data)
                        .into_response());
                }
            }
        }
        Err(MountError::PathNotFile(_)) => {}
        Err(e) => return Err(GetContentError::Mount(e)),
    }

    let ls_result = mount.ls(&path).await;
    let ls = match ls_result {
        Ok(ls) => {
            if ls.is_empty() {
                tracing::info!(
                    "GET {} | {:?} | returning 404 - empty dir",
                    path.display(),
                    query
                );
                return Err(GetContentError::NotFound);
            }
            ls
        }
        Err(MountError::PathNotDir(_)) => {
            tracing::info!(
                "GET {} | {:?} | returning 404 - not path",
                path.display(),
                query
            );
            return Err(GetContentError::NotFound);
        }
        Err(e) => return Err(GetContentError::Mount(e)),
    };
    tracing::info!(
        "GET {} | {:?} | returning ls: {:?}",
        path.display(),
        query,
        ls
    );
    let ls_json = serde_json::to_string(&ls).unwrap();

    Ok((http::StatusCode::OK, Json(ls_json)).into_response())
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
            GetContentError::Mount(_)
            | GetContentError::RootCid(_)
            | GetContentError::Database(_) => {
                tracing::error!("{:?}", self);
                (
                    http::StatusCode::INTERNAL_SERVER_ERROR,
                    "unknown server error",
                )
                    .into_response()
            }
            GetContentError::RootNotFound => {
                (http::StatusCode::NOT_FOUND, "No root CID found").into_response()
            }
            GetContentError::NotFound => (http::StatusCode::NOT_FOUND, "Not found").into_response(),
        }
    }
}

pub fn markdown_to_html(data: Vec<u8>, base_path: &PathBuf, get_content_url: &Url) -> String {
    let content = String::from_utf8(data).unwrap();

    let mut options = pulldown_cmark::Options::empty();
    options.insert(pulldown_cmark::Options::ENABLE_STRIKETHROUGH);
    options.insert(pulldown_cmark::Options::ENABLE_TASKLISTS);
    options.insert(pulldown_cmark::Options::ENABLE_TABLES);
    options.insert(pulldown_cmark::Options::ENABLE_FOOTNOTES);
    options.insert(pulldown_cmark::Options::ENABLE_SMART_PUNCTUATION);
    options.insert(pulldown_cmark::Options::ENABLE_TASKLISTS);

    let parser = pulldown_cmark::Parser::new_ext(&content, options);
    let mut html = String::new();
    pulldown_cmark::html::push_html(&mut html, parser);

    let re = Regex::new(r#"src="./([^"]+)"#).unwrap();
    let mut result = html.clone();

    for caps in re.captures_iter(&html) {
        if let Some(cap) = caps.get(1) {
            let path = PathBuf::from(cap.as_str());
            let path = normalize_path(base_path.join(path));
            let url = get_content_url.join(path.to_str().unwrap()).unwrap();
            let old = format!(r#"src="./{}""#, cap.as_str());
            let new = format!(r#"src="{}""#, url);
            result = result.replace(&old, &new);
        }
    }

    result
}

fn normalize_path(path: PathBuf) -> PathBuf {
    let mut normalized_path = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::ParentDir => {
                normalized_path.pop();
            }
            _ => {
                normalized_path.push(component);
            }
        }
    }
    normalized_path
}
