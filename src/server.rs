use crate::config::ProxyConfig;
use crate::logging::SharedLogger;
use crate::proxy;
use crate::translate::anthropic_types::{ErrorResponse, MessagesRequest};

use axum::body::Body;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::sse::{Event, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use bytes::Bytes;
use futures::stream::StreamExt;
use std::convert::Infallible;
use std::sync::Arc;
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;

#[derive(Clone)]
pub struct AppState {
    pub config: ProxyConfig,
    pub client: reqwest::Client,
    pub logger: SharedLogger,
}

pub fn build_router(state: Arc<AppState>) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    Router::new()
        .route("/v1/messages", post(handle_messages))
        .route("/health", get(handle_health))
        .route("/v1/models", get(handle_models))
        .layer(cors)
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

async fn handle_messages(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    // Anthropic passthrough mode (no translation needed)
    if state.config.is_anthropic_format() {
        return handle_passthrough(state, headers, body).await;
    }

    // Parse the Anthropic request
    let req: MessagesRequest = match serde_json::from_slice(&body) {
        Ok(r) => r,
        Err(e) => {
            state.logger.error(
                "server",
                format!("Failed to parse request: {}", e),
            );
            let err = ErrorResponse::invalid_request(format!("Invalid request body: {}", e));
            return (StatusCode::BAD_REQUEST, Json(err)).into_response();
        }
    };

    let is_streaming = req.stream.unwrap_or(false);

    state.logger.info(
        "server",
        format!(
            "Request: model={} streaming={} messages={}",
            req.model,
            is_streaming,
            req.messages.len()
        ),
    );

    if is_streaming {
        handle_streaming(state, &req).await
    } else {
        handle_non_streaming(state, &req).await
    }
}

async fn handle_non_streaming(state: Arc<AppState>, req: &MessagesRequest) -> Response {
    match proxy::proxy_non_streaming(req, &state.config, &state.client, &state.logger).await {
        Ok(proxy::ProxyResult::Success(resp)) => Json(resp).into_response(),
        Ok(proxy::ProxyResult::Error(err, status_code)) => {
            let status = StatusCode::from_u16(status_code).unwrap_or(StatusCode::BAD_GATEWAY);
            (status, Json(err)).into_response()
        }
        Err(e) => {
            state.logger.error("server", format!("Proxy error: {}", e));
            let err = ErrorResponse::api_error(format!("Proxy error: {}", e));
            (StatusCode::BAD_GATEWAY, Json(err)).into_response()
        }
    }
}

async fn handle_streaming(state: Arc<AppState>, req: &MessagesRequest) -> Response {
    let sse_stream =
        match proxy::proxy_streaming(req, &state.config, &state.client, &state.logger).await {
            Ok(s) => s,
            Err(e) => {
                state.logger.error("server", format!("Streaming setup error: {}", e));
                let err = ErrorResponse::api_error(format!("Streaming error: {}", e));
                return (StatusCode::BAD_GATEWAY, Json(err)).into_response();
            }
        };

    let event_stream = sse_stream.map(|result| -> std::result::Result<Event, Infallible> {
        match result {
            Ok(sse_event) => Ok(Event::default()
                .event(sse_event.event)
                .data(sse_event.data)),
            Err(_) => Ok(Event::default().event("error").data("{}")),
        }
    });

    Sse::new(event_stream)
        .keep_alive(axum::response::sse::KeepAlive::default())
        .into_response()
}

async fn handle_passthrough(
    state: Arc<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let req_headers = reqwest_headers_from_axum(&headers);

    match proxy::proxy_passthrough(body, &req_headers, &state.config, &state.client, &state.logger)
        .await
    {
        Ok((status, _resp_headers, resp_body)) => {
            let status_code = StatusCode::from_u16(status).unwrap_or(StatusCode::BAD_GATEWAY);

            // Check if response is streaming (SSE)
            let content_type = _resp_headers
                .get("content-type")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("");

            if content_type.contains("text/event-stream") {
                Response::builder()
                    .status(status_code)
                    .header("content-type", "text/event-stream")
                    .header("cache-control", "no-cache")
                    .body(Body::from(resp_body))
                    .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
            } else {
                Response::builder()
                    .status(status_code)
                    .header("content-type", "application/json")
                    .body(Body::from(resp_body))
                    .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
            }
        }
        Err(e) => {
            state.logger.error("server", format!("Passthrough error: {}", e));
            let err = ErrorResponse::api_error(format!("Passthrough error: {}", e));
            (StatusCode::BAD_GATEWAY, Json(err)).into_response()
        }
    }
}

async fn handle_health() -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "status": "ok",
        "version": env!("CARGO_PKG_VERSION"),
    }))
}

async fn handle_models(State(state): State<Arc<AppState>>) -> Json<serde_json::Value> {
    let models: Vec<serde_json::Value> = state
        .config
        .models
        .keys()
        .map(|name| {
            serde_json::json!({
                "id": name,
                "object": "model",
                "owned_by": state.config.provider.name,
            })
        })
        .collect();

    Json(serde_json::json!({ "data": models, "object": "list" }))
}

fn reqwest_headers_from_axum(headers: &HeaderMap) -> reqwest::header::HeaderMap {
    let mut out = reqwest::header::HeaderMap::new();
    for (key, value) in headers.iter() {
        if let Ok(name) = reqwest::header::HeaderName::from_bytes(key.as_str().as_bytes()) {
            if let Ok(val) = reqwest::header::HeaderValue::from_bytes(value.as_bytes()) {
                out.insert(name, val);
            }
        }
    }
    out
}
