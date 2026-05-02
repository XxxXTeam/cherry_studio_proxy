use axum::{
    extract::Request,
    http::{header, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};

use crate::config::Settings;

pub async fn auth_middleware(
    request: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    let settings = request
        .extensions()
        .get::<Settings>()
        .cloned()
        .unwrap_or_default();

    if !request.uri().path().starts_with("/v1/") {
        return Ok(next.run(request).await);
    }

    if let Some(expected_key) = settings.openai_api_key {
        let authorized = request
            .headers()
            .get(header::AUTHORIZATION)
            .and_then(|h| h.to_str().ok())
            .map(|header| {
                let prefix = "Bearer ";
                if let Some(token) = header.strip_prefix(prefix) {
                    token.trim() == expected_key
                } else {
                    false
                }
            })
            .unwrap_or(false);

        if !authorized {
            return Ok(openai_error(
                "Invalid or missing API key.",
                StatusCode::UNAUTHORIZED,
                "authentication_error",
                Some("invalid_api_key"),
            )
            .into_response());
        }
    }

    Ok(next.run(request).await)
}

pub fn openai_error(
    message: &str,
    status: StatusCode,
    error_type: &str,
    code: Option<&str>,
) -> impl IntoResponse {
    let body = serde_json::json!({
        "error": {
            "message": message,
            "type": error_type,
            "code": code,
        }
    });
    (status, axum::Json(body))
}
