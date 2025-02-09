use axum::extract::{Json, Path as AxumPath, Query, State};
use axum::http::header::CONTENT_TYPE;
use axum::response::{IntoResponse, Response};
use image::{imageops::FilterType, ImageFormat};
use regex::Regex;
use std::io::Cursor;
use std::path::Path;
use std::path::PathBuf;
use std::time::Duration;
use tokio::time::timeout;
use url::Url;

use leaky_common::prelude::*;

use crate::app::AppState;
use crate::database::models::RootCid;

const MAX_WIDTH: u32 = 300;
const MAX_HEIGHT: u32 = 300;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug, serde::Deserialize)]
pub struct GetContentQuery {
    pub html: Option<bool>,
    pub thumbnail: Option<bool>,
}

#[derive(Debug, serde::Serialize)]
struct Item {
    cid: String,
    path: String,
    is_dir: bool,
    object: Option<Object>,
}

#[derive(Debug, serde::Serialize)]
struct LsResponse(Vec<Item>);

pub async fn handler(
    State(state): State<AppState>,
    AxumPath(path): AxumPath<PathBuf>,
    Query(query): Query<GetContentQuery>,
) -> Result<impl IntoResponse, GetContentError> {
    let path_clone = path.clone();
    tracing::debug!("Starting content request for path: {:?}", path_clone);

    let result = timeout(REQUEST_TIMEOUT, async move {
        tracing::debug!("acquiring mount guard");
        let mount_guard = state.mount_guard();

        // Make the path absolute
        let path = PathBuf::from("/").join(path);

        // TODO: add formatting for html requests
        let ls_result = mount_guard.ls(&path).await;
        match ls_result {
            Ok((ls, _)) => {
                if !ls.is_empty() {
                    return Ok((
                        http::StatusCode::OK,
                        [(CONTENT_TYPE, "application/json")],
                        Json(LsResponse(
                            ls.into_iter()
                                .map(|(path, link)| Item {
                                    cid: link.cid().to_string(),
                                    path: path.to_str().unwrap().to_string(),
                                    is_dir: match link {
                                        NodeLink::Node(_) => true,
                                        NodeLink::Data(_, _) => false,
                                    },
                                    object: match link {
                                        NodeLink::Node(_) => None,
                                        NodeLink::Data(_, object) => object,
                                    },
                                })
                                .collect(),
                        )),
                    )
                        .into_response());
                }
            }
            Err(MountError::PathNotDir(_)) => {}
            Err(MountError::PathNotFound(_)) => {
                return Err(GetContentError::NotFound);
            }
            Err(e) => return Err(GetContentError::Mount(e)),
        };

        let ext = path
            .extension()
            .unwrap_or_default()
            .to_str()
            .unwrap_or_default();

        match ext {
            // Markdown
            "md" => {
                if query.html.unwrap_or(false) {
                    let base_path = path.parent().unwrap_or_else(|| Path::new(""));
                    let get_content_url =
                        state.get_content_forwarding_url().join("content").unwrap();

                    let data = mount_guard
                        .cat(&path)
                        .await
                        .map_err(|_| GetContentError::NotFound)?;

                    let html = markdown_to_html(data, base_path, &get_content_url);
                    Ok((http::StatusCode::OK, [(CONTENT_TYPE, "text/html")], html).into_response())
                } else {
                    let data = mount_guard
                        .cat(&path)
                        .await
                        .map_err(|_| GetContentError::NotFound)?;

                    Ok(
                        (http::StatusCode::OK, [(CONTENT_TYPE, "text/plain")], data)
                            .into_response(),
                    )
                }
            }
            // Images
            "png" | "jpg" | "jpeg" | "gif" => {
                let data = mount_guard
                    .cat(&path)
                    .await
                    .map_err(|_| GetContentError::NotFound)?;
                if query.thumbnail.unwrap_or(false) && ext != "gif" {
                    let resized_image = resize_image(&data, ext)?;
                    Ok((
                        http::StatusCode::OK,
                        [(CONTENT_TYPE, format!("image/{}", ext))],
                        resized_image,
                    )
                        .into_response())
                } else {
                    Ok((
                        http::StatusCode::OK,
                        [(CONTENT_TYPE, format!("image/{}", ext))],
                        data,
                    )
                        .into_response())
                }
            }
            // All other files
            _ => {
                let data = mount_guard
                    .cat(&path)
                    .await
                    .map_err(|_| GetContentError::NotFound)?;
                Ok((
                    http::StatusCode::OK,
                    [(CONTENT_TYPE, "application/octet-stream")],
                    data,
                )
                    .into_response())
            }
        }
    })
    .await
    .map_err(|_| GetContentError::Timeout)?;

    tracing::debug!("completed content request for path: {:?}", path_clone);
    result
}

fn resize_image(img_data: &[u8], format: &str) -> Result<Vec<u8>, GetContentError> {
    let img = image::load_from_memory(img_data)
        .map_err(|e| GetContentError::ImageProcessing(e.to_string()))?;

    let (width, height) = calculate_dimensions(img.width(), img.height());
    let resized = img.resize(width, height, FilterType::Lanczos3);

    let mut cursor = Cursor::new(Vec::new());
    let format = match format {
        "png" => ImageFormat::Png,
        "jpg" | "jpeg" => ImageFormat::Jpeg,
        _ => return Err(GetContentError::UnsupportedImageFormat),
    };

    resized
        .write_to(&mut cursor, format)
        .map_err(|e| GetContentError::ImageProcessing(e.to_string()))?;

    Ok(cursor.into_inner())
}

fn calculate_dimensions(width: u32, height: u32) -> (u32, u32) {
    let aspect_ratio = width as f32 / height as f32;
    if width > height {
        let new_width = MAX_WIDTH.min(width);
        let new_height = (new_width as f32 / aspect_ratio) as u32;
        (new_width, new_height)
    } else {
        let new_height = MAX_HEIGHT.min(height);
        let new_width = (new_height as f32 * aspect_ratio) as u32;
        (new_width, new_height)
    }
}

pub fn markdown_to_html(data: Vec<u8>, base_path: &Path, get_content_url: &Url) -> String {
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
    #[error("Image processing error: {0}")]
    ImageProcessing(String),
    #[error("Unsupported image format")]
    UnsupportedImageFormat,
    #[error("Request timed out")]
    Timeout,
}

impl IntoResponse for GetContentError {
    fn into_response(self) -> Response {
        match self {
            GetContentError::Mount(_)
            | GetContentError::RootCid(_)
            | GetContentError::Database(_)
            | GetContentError::ImageProcessing(_) => {
                tracing::error!("{:?}", self);
                (
                    http::StatusCode::INTERNAL_SERVER_ERROR,
                    [(CONTENT_TYPE, "text/plain")],
                    "Internal server error",
                )
                    .into_response()
            }
            GetContentError::RootNotFound | GetContentError::NotFound => (
                http::StatusCode::NOT_FOUND,
                [(CONTENT_TYPE, "text/plain")],
                "Not found",
            )
                .into_response(),
            GetContentError::UnsupportedImageFormat => (
                http::StatusCode::UNSUPPORTED_MEDIA_TYPE,
                [(CONTENT_TYPE, "text/plain")],
                "Unsupported image format",
            )
                .into_response(),
            GetContentError::Timeout => (
                http::StatusCode::REQUEST_TIMEOUT,
                [(CONTENT_TYPE, "text/plain")],
                "Request timed out",
            )
                .into_response(),
        }
    }
}
