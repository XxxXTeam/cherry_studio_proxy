use axum::{
    body::{Body, Bytes},
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use futures::StreamExt;
use serde_json::{json, Value};
use std::sync::Arc;

use crate::{
    auth::openai_error,
    config::Settings,
    mappers::{format_stream_line_as_sse, to_cherry_payload},
    upstream::{CherryClient, CherryUpstreamError},
};

#[derive(Clone)]
pub struct AppState {
    pub settings: Settings,
    pub cherry_client: CherryClient,
}

pub async fn index(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let models: Vec<String> = state.settings.cherry_models.keys().cloned().collect();
    Json(json!({
        "name": "cherry-openai-proxy",
        "object": "service",
        "message": "OpenAI-compatible proxy for Cherry chat completions.",
        "endpoints": [
            "/v1/chat/completions",
            "/v1/models",
            "/health",
        ],
        "model": state.settings.default_public_model(),
        "models": models,
    }))
}

pub async fn health() -> impl IntoResponse {
    Json(json!({"status": "ok"}))
}

pub async fn list_models(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let data: Vec<Value> = state
        .settings
        .cherry_models
        .keys()
        .map(|public_model| {
            json!({
                "id": public_model,
                "object": "model",
                "owned_by": state.settings.model_owner(),
            })
        })
        .collect();

    Json(json!({
        "object": "list",
        "data": data,
    }))
}

pub async fn chat_completions(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<Value>,
) -> Response {
    if !payload.is_object() {
        return openai_error(
            "Request body must be a JSON object.",
            StatusCode::BAD_REQUEST,
            "invalid_request_error",
            Some("invalid_json"),
        )
        .into_response();
    }

    let model = payload.get("model").and_then(|m| m.as_str());
    let model = match model {
        Some(m) if !m.trim().is_empty() => m,
        _ => {
            return openai_error(
                "'model' must be a non-empty string.",
                StatusCode::BAD_REQUEST,
                "invalid_request_error",
                Some("invalid_model"),
            )
            .into_response();
        }
    };

    let upstream_model = state.settings.resolve_upstream_model(model).unwrap_or_else(|| model.to_string());

    let upstream_payload = match to_cherry_payload(&payload, &upstream_model) {
        Ok(p) => p,
        Err(e) => {
            return openai_error(
                &e,
                StatusCode::BAD_REQUEST,
                "invalid_request_error",
                Some("invalid_request"),
            )
            .into_response();
        }
    };

    let stream = payload.get("stream").and_then(|s| s.as_bool()).unwrap_or(false);

    if stream {
        handle_stream(state, upstream_payload).await
    } else {
        handle_non_stream(state, upstream_payload).await
    }
}

async fn handle_non_stream(state: Arc<AppState>, upstream_payload: Value) -> Response {
    match state.cherry_client.create_chat_completion(&upstream_payload).await {
        Ok(body) => (StatusCode::OK, Json(body)).into_response(),
        Err(e) => upstream_error_response(e),
    }
}

async fn handle_stream(state: Arc<AppState>, upstream_payload: Value) -> Response {
    let upstream_response = match state.cherry_client.stream_chat_completion(&upstream_payload).await {
        Ok(resp) => resp,
        Err(e) => return upstream_error_response(e),
    };

    let content_type = upstream_response
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("text/event-stream")
        .to_string();

    let is_sse = content_type.starts_with("text/event-stream");
    let log_sse = state.settings.log_sse_stream;

    let stream = upstream_response.bytes_stream();

    let body = if is_sse {
        build_sse_passthrough_stream(stream, log_sse)
    } else {
        build_sse_formatted_stream(stream, log_sse)
    };

    Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "text/event-stream; charset=utf-8")
        .header("Cache-Control", "no-cache")
        .header("Connection", "keep-alive")
        .header("X-Accel-Buffering", "no")
        .body(body)
        .unwrap()
}

fn build_sse_passthrough_stream<S>(stream: S, log_sse: bool) -> Body
where
    S: futures::Stream<Item = Result<Bytes, reqwest::Error>> + Send + 'static,
{
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    let seen_done = Arc::new(AtomicBool::new(false));
    let seen_done_clone = Arc::clone(&seen_done);

    let mapped = stream.map(move |result| {
        match result {
            Ok(chunk) => {
                if log_sse {
                    let text = String::from_utf8_lossy(&chunk);
                    tracing::info!("SSE upstream raw chunk={}", safe_log_text(&text));
                }
                if !seen_done_clone.load(Ordering::SeqCst) {
                    let text = String::from_utf8_lossy(&chunk);
                    if text.contains("data: [DONE]") {
                        seen_done_clone.store(true, Ordering::SeqCst);
                    }
                }
                Ok::<_, std::convert::Infallible>(chunk)
            }
            Err(e) => {
                tracing::error!("Streaming passthrough failed: {}", e);
                Ok(Bytes::new())
            }
        }
    });

    let done_stream = futures::stream::once(async move {
        if !seen_done.load(Ordering::SeqCst) {
            Ok::<_, std::convert::Infallible>(Bytes::from_static(b"data: [DONE]\n\n"))
        } else {
            Ok(Bytes::new())
        }
    });

    Body::from_stream(mapped.chain(done_stream))
}

fn build_sse_formatted_stream<S>(stream: S, log_sse: bool) -> Body
where
    S: futures::Stream<Item = Result<Bytes, reqwest::Error>> + Send + 'static,
{
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    let seen_done = Arc::new(AtomicBool::new(false));
    let seen_done_clone = Arc::clone(&seen_done);

    let stream = stream.map(move |result| {
        match result {
            Ok(chunk) => {
                let text = String::from_utf8_lossy(&chunk);
                if log_sse {
                    tracing::info!("SSE upstream line={}", safe_log_text(&text));
                }
                let mut output = Vec::new();
                for line in text.lines() {
                    if let Some(normalized) = format_stream_line_as_sse(line) {
                        if log_sse {
                            tracing::info!("SSE proxy line={}", safe_log_text(&normalized));
                        }
                        if normalized.trim() == "data: [DONE]" {
                            seen_done_clone.store(true, Ordering::SeqCst);
                        }
                        output.extend_from_slice(normalized.as_bytes());
                    }
                }
                Ok::<_, std::convert::Infallible>(Bytes::from(output))
            }
            Err(e) => {
                tracing::error!("Streaming proxy failed: {}", e);
                Ok(Bytes::new())
            }
        }
    });

    let done_stream = futures::stream::once(async move {
        if !seen_done.load(Ordering::SeqCst) {
            Ok::<_, std::convert::Infallible>(Bytes::from_static(b"data: [DONE]\n\n"))
        } else {
            Ok(Bytes::new())
        }
    });

    Body::from_stream(stream.chain(done_stream))
}

fn upstream_error_response(error: CherryUpstreamError) -> Response {
    if let Some(body) = error.body {
        let ct = error.content_type.unwrap_or_else(|| "application/json; charset=utf-8".to_string());
        return Response::builder()
            .status(error.status_code)
            .header("content-type", ct)
            .body(Body::from(body.to_string()))
            .unwrap();
    }

    openai_error(
        &error.message,
        StatusCode::from_u16(error.status_code).unwrap_or(StatusCode::BAD_GATEWAY),
        &error.error_type,
        error.code.as_deref(),
    )
    .into_response()
}

fn safe_log_text(text: &str) -> String {
    const LIMIT: usize = 1500;
    if text.len() <= LIMIT {
        text.to_string()
    } else {
        format!("{}...<truncated>", &text[..LIMIT])
    }
}
