use std::convert::TryFrom;

use axum::{
    body::Body,
    http::{Request, Response, StatusCode, Uri},
};
use hyper::client::Client;
use url::Url;

pub struct IpfsApiProxy(Url);

impl IpfsApiProxy {
    pub fn new(base_url: Url) -> Self {
        Self(base_url)
    }
    pub async fn handle(&self, req: Request<Body>) -> Result<Response<Body>, StatusCode> {
        // Extract the path and query from the incoming request
        let path_and_query = req
            .uri()
            .path_and_query()
            .map(|pq| pq.as_str())
            .unwrap_or("/");
        let uri = format!("{}{}", self.0, path_and_query);
        // Update the URI of the request
        let mut req = req;
        *req.uri_mut() = Uri::try_from(uri).map_err(|_| StatusCode::BAD_REQUEST)?;
        // Create a Hyper client
        let client = Client::new();
        // Forward the request to the Kubo RPC API
        let res = client
            .request(req)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        Ok(res)
    }
}
