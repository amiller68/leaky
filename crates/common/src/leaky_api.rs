use std::str::FromStr;

use serde::{Deserialize, Serialize};
use url::Url;

use crate::types::Cid;

/* Ipfs Rpc Client Wrapper */

#[derive(Clone)]
pub struct LeakyApi {
    base_url: Url,
    client: reqwest::Client,
}

impl Default for LeakyApi {
    fn default() -> Self {
        let url: Url = "http://localhost:3000".try_into().unwrap();
        Self::try_from(url).unwrap()
    }
}

// TODO: make this less convoluted
impl TryFrom<Url> for LeakyApi {
    type Error = LeakyApiError;
    fn try_from(base_url: Url) -> Result<Self, LeakyApiError> {
        let mut default_headers = reqwest::header::HeaderMap::new();
        default_headers.insert("content-type", "application/json".parse().unwrap());
        let client = reqwest::Client::builder()
            .default_headers(default_headers)
            .build()
            .map_err(LeakyApiError::Client)?;
        Ok(Self { base_url, client })
    }
}

#[derive(Debug, Serialize)]
pub struct PushRootRequest {
    cid: String,
    previous_cid: String,
}

#[derive(Debug, Deserialize)]
pub struct PullRootResponse {
    cid: String,
}

impl LeakyApi {
    pub async fn push_root(&self, cid: &Cid, previous_cid: &Cid) -> Result<(), LeakyApiError> {
        let url = self.base_url.join("api/v0/root")?;
        let body = serde_json::to_string(&PushRootRequest {
            cid: cid.to_string(),
            previous_cid: previous_cid.to_string(),
        })?;
        // push_root
        let response = self.client.post(url).body(body).send().await?;
        if !response.status().is_success() {
            return Err(LeakyApiError::Api(
                response.status(),
                response.text().await?,
            ));
        }
        Ok(())
    }

    pub async fn pull_root(&self) -> Result<Cid, LeakyApiError> {
        let url = self.base_url.join("api/v0/root")?;
        let response = self.client.get(url).send().await?;
        let response = response.text().await?;
        let response: PullRootResponse = serde_json::from_str(&response)?;
        Ok(Cid::from_str(&response.cid).unwrap())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum LeakyApiError {
    #[error("default error: {0}")]
    Default(#[from] anyhow::Error),
    #[error("url parse error")]
    Url(#[from] url::ParseError),
    #[error("http error")]
    Http(#[from] http::Error),
    #[error("Failed to parse scheme")]
    Scheme(#[from] http::uri::InvalidUri),
    #[error("Failed to build client: {0}")]
    Client(#[from] reqwest::Error),
    #[error("Serde error: {0}")]
    Serde(#[from] serde_json::Error),
    #[error("Api error: {0} {1}")]
    Api(reqwest::StatusCode, String),
}

/*
#[cfg(test)]
mod tests {}
*/
