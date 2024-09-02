#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    #[error("auth required")]
    AuthRequired,
    #[error("invalid url: {0}")]
    Url(#[from] url::ParseError),
    #[error("reqwest error: {0}")]
    Reqwest(#[from] reqwest::Error),
    #[error("status code: {0}")]
    HttpStatus(reqwest::StatusCode),
    #[error("sthumbs up: {0}")]
    ThumbsUp(#[from] thumbs_up::prelude::KeyError),
    #[error("boxed request error: {0}")]
    Box(#[from] Box<dyn std::error::Error + Send + Sync>),
}
