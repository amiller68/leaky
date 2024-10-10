use axum::extract::{Json, Path, Query, State};
use axum::http::header::CONTENT_TYPE;
use axum::response::{IntoResponse, Response};
use image::{imageops::FilterType, ImageFormat};
use std::path::PathBuf;
use std::io::Cursor;
use url::Url;
use regex::Regex;

use leaky_common::prelude::*;

use crate::app::AppState;
use crate::database::models::RootCid;

const MAX_WIDTH: u32 = 300;
const MAX_HEIGHT: u32 = 300;

#[derive(Debug, serde::Deserialize)]
pub struct GetContentQuery {
    pub html: Option<bool>,
    pub thumbnail: Option<bool>,
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

    let ls_result = mount.ls(&path).await;
    match ls_result {
        Ok(ls) => {
            if !ls.is_empty() {
                tracing::info!(
                    "GET {} | {:?} | returning ls: {:?}",
                    path.display(),
                    query,
                    ls
                );
                return Ok((http::StatusCode::OK, Json(ls)).into_response());
            }
        }
        Err(MountError::PathNotDir(_)) => {}
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
                tracing::info!(
                    "GET {} | {:?} | rendering markdown as html",
                    path.display(),
                    query
                );
                let base_path = path.parent();
                let empty_path = PathBuf::new();
                let base_path = base_path.unwrap_or(&empty_path);
                let get_content_url = state.get_content_forwarding_url().join("content").unwrap();
                let data = mount.cat(&path).await.map_err(|_| GetContentError::NotFound)?;
                let html = markdown_to_html(data, &base_path.to_path_buf(), &get_content_url);
                Ok((http::StatusCode::OK, [(CONTENT_TYPE, "text/html")], html).into_response())
            } else {
                tracing::info!(
                    "GET {} | {:?} | returning markdown as text",
                    path.display(),
                    query
                );
                let data = mount.cat(&path).await.map_err(|_| GetContentError::NotFound)?;
                Ok((http::StatusCode::OK, [(CONTENT_TYPE, "text/plain")], data).into_response())
            }
        }
        // Images
        "png" | "jpg" | "jpeg" | "gif" => {
            tracing::info!("GET {} | {:?} | returning image", path.display(), query);
            let data = mount.cat(&path).await.map_err(|_| GetContentError::NotFound)?;
            if query.thumbnail.unwrap_or(false) && ext != "gif" {
                let resized_image = resize_image(&data, ext)?;
                Ok((http::StatusCode::OK, [(CONTENT_TYPE, format!("image/{}", ext))], resized_image).into_response())
            } else {
                Ok((http::StatusCode::OK, [(CONTENT_TYPE, format!("image/{}", ext))], data).into_response())
            }
        }
        // All other files
        _ => {
            tracing::info!("GET {} | {:?} | returning misc file", path.display(), query);
            let data = mount.cat(&path).await.map_err(|_| GetContentError::NotFound)?;
            Ok((http::StatusCode::OK, [(CONTENT_TYPE, "application/octet-stream")], data).into_response())
        }
    }
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
            tracing::info!("replacing {} with {}", cap.as_str(), url);
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
}

impl IntoResponse for GetContentError {
    fn into_response(self) -> Response {
        match self {
            GetContentError::Mount(_)
            | GetContentError::RootCid(_)
            | GetContentError::Database(_)
            | GetContentError::ImageProcessing(_) => {
                tracing::error!("{:?}", self);
                (http::StatusCode::INTERNAL_SERVER_ERROR, "Internal server error").into_response()
            }
            GetContentError::RootNotFound | GetContentError::NotFound => {
                (http::StatusCode::NOT_FOUND, "Not found").into_response()
            }
            GetContentError::UnsupportedImageFormat => {
                (http::StatusCode::UNSUPPORTED_MEDIA_TYPE, "Unsupported image format").into_response()
            }
        }
    }
}