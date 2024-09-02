use axum::body::Body;
use axum::error_handling::HandleErrorLayer;
use axum::extract::{DefaultBodyLimit, State};
use axum::handler::HandlerWithoutStateExt;
use axum::http::{StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::Router;
use axum::ServiceExt;
use axum_extra::headers::ContentType;
use axum_extra::TypedHeader;
use http::uri::PathAndQuery;
use http::{header, Request};
use leptos::*;
use leptos_axum::{generate_route_list, LeptosRoutes};
use std::path::PathBuf;
use time::OffsetDateTime;
use tokio::sync::watch;
use tower::ServiceBuilder;
use tower_http::sensitive_headers::{
    SetSensitiveRequestHeadersLayer, SetSensitiveResponseHeadersLayer,
};
use tower_http::services::ServeDir;
use tower_http::trace::{DefaultOnFailure, DefaultOnResponse, MakeSpan, TraceLayer};
use tower_http::{LatencyUnit, ServiceBuilderExt};
use tracing::{Level, Span};

async fn handler(
    uri: Uri,
    State(state): State<AppState>,
    req: Request<Body>,
    TypedHeader(content_type): TypedHeader<ContentType>,
) -> impl IntoResponse {
    let maybe_bytes = ipfs::serve_root(uri, &state).await;
    let bytes = match maybe_bytes {
        Ok(bytes) => bytes,
        Err(e) => match e {
            ipfs::IpfsServeError::MissingRootCid
            | ipfs::IpfsServeError::MissingIpfsContent(_, _) => {
                // Pass through to the not found handler
                return error_handlers::redirect_to_app(&state, req)
                    .await
                    .into_response();
            }
            _ => return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
        },
    };
    let response = Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, content_type.to_string())
        .body(Body::from(bytes))
        .expect("response builder to succeed");

    response
}
