use reqwest::{Client, Response as ReqwestResponse};
use serde_json::Value;
use std::time::Duration;

use crate::config::Settings;
use crate::signatures::{generate_signature_headers, serialize_body};

#[derive(Debug)]
pub struct CherryUpstreamError {
    pub message: String,
    pub status_code: u16,
    pub error_type: String,
    pub code: Option<String>,
    pub body: Option<Value>,
    pub content_type: Option<String>,
}

impl std::fmt::Display for CherryUpstreamError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for CherryUpstreamError {}

#[derive(Clone, Debug)]
pub struct CherryClient {
    settings: Settings,
    http_client: Client,
}

impl CherryClient {
    pub fn new(settings: Settings) -> Self {
        let timeout = Duration::from_secs_f64(settings.request_timeout);
        let connect_timeout = Duration::from_secs_f64(settings.request_timeout.min(10.0));

        let http_client = Client::builder()
            .timeout(timeout)
            .connect_timeout(connect_timeout)
            .build()
            .unwrap_or_default();

        Self {
            settings,
            http_client,
        }
    }

    pub async fn create_chat_completion(&self, payload: &Value) -> Result<Value, CherryUpstreamError> {
        let body_string = serialize_body(Some(payload));
        let response = self
            .request("POST", self.settings.chat_completions_path(), payload, &body_string)
            .await?;
        self.parse_json_response(response).await
    }

    pub async fn stream_chat_completion(
        &self,
        payload: &Value,
    ) -> Result<ReqwestResponse, CherryUpstreamError> {
        let body_string = serialize_body(Some(payload));
        let path = self.settings.chat_completions_path();
        let url = format!("{}{}", self.settings.cherry_base_url, path);
        let headers = self.build_headers(path, payload);

        let mut request = self.http_client.post(&url);
        for (key, value) in headers {
            request = request.header(key, value);
        }

        let response = match request.body(body_string).send().await {
            Ok(resp) => resp,
            Err(e) if e.is_timeout() => {
                return Err(CherryUpstreamError {
                    message: "Upstream request timed out.".to_string(),
                    status_code: 504,
                    error_type: "timeout_error".to_string(),
                    code: Some("upstream_timeout".to_string()),
                    body: None,
                    content_type: None,
                });
            }
            Err(_) => {
                return Err(CherryUpstreamError {
                    message: "Failed to connect to Cherry upstream.".to_string(),
                    status_code: 502,
                    error_type: "api_connection_error".to_string(),
                    code: Some("upstream_connection_error".to_string()),
                    body: None,
                    content_type: None,
                });
            }
        };

        if response.status().as_u16() >= 400 {
            let error = self.error_from_response(response).await;
            return Err(error);
        }

        Ok(response)
    }

    async fn request(
        &self,
        method: &str,
        path: &str,
        payload: &Value,
        body_string: &str,
    ) -> Result<ReqwestResponse, CherryUpstreamError> {
        let url = format!("{}{}", self.settings.cherry_base_url, path);
        let headers = self.build_headers(path, payload);

        let mut request = self.http_client.request(reqwest::Method::from_bytes(method.as_bytes()).unwrap(), &url);
        for (key, value) in headers {
            request = request.header(key, value);
        }

        let response = match request.body(body_string.to_string()).send().await {
            Ok(resp) => resp,
            Err(e) if e.is_timeout() => {
                return Err(CherryUpstreamError {
                    message: "Upstream request timed out.".to_string(),
                    status_code: 504,
                    error_type: "timeout_error".to_string(),
                    code: Some("upstream_timeout".to_string()),
                    body: None,
                    content_type: None,
                });
            }
            Err(_) => {
                return Err(CherryUpstreamError {
                    message: "Failed to connect to Cherry upstream.".to_string(),
                    status_code: 502,
                    error_type: "api_connection_error".to_string(),
                    code: Some("upstream_connection_error".to_string()),
                    body: None,
                    content_type: None,
                });
            }
        };

        if response.status().as_u16() >= 400 {
            let error = self.error_from_response(response).await;
            return Err(error);
        }

        Ok(response)
    }

    fn build_headers(&self, path: &str, payload: &Value) -> Vec<(&'static str, String)> {
        let signed_headers = generate_signature_headers("POST", path, "", Some(payload), None);

        let mut headers = vec![
            ("Accept", "application/json, text/event-stream".to_string()),
            ("Content-Type", "application/json".to_string()),
            ("User-Agent", self.settings.cherry_user_agent.clone()),
            ("X-Title", self.settings.cherry_title.clone()),
            ("HTTP-Referer", self.settings.cherry_referer.clone()),
        ];

        for (k, v) in signed_headers {
            headers.push((k, v));
        }

        headers
    }

    async fn parse_json_response(&self, response: ReqwestResponse) -> Result<Value, CherryUpstreamError> {
        match response.json().await {
            Ok(json) => Ok(json),
            Err(_) => Err(CherryUpstreamError {
                message: "Cherry upstream returned invalid JSON.".to_string(),
                status_code: 502,
                error_type: "api_error".to_string(),
                code: Some("invalid_upstream_json".to_string()),
                body: None,
                content_type: None,
            }),
        }
    }

    async fn error_from_response(&self, response: ReqwestResponse) -> CherryUpstreamError {
        let status_code = response.status().as_u16();
        let content_type = response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());

        let mut message = format!("Cherry upstream returned HTTP {}.", status_code);
        let mut error_type = "api_error".to_string();
        let mut code: Option<String> = None;
        let mut body: Option<Value> = None;

        let text = response.text().await.unwrap_or_default();
        if let Ok(payload) = serde_json::from_str::<Value>(&text) {
            body = Some(payload.clone());
            if let Some(Value::Object(error)) = payload.get("error") {
                if let Some(Value::String(m)) = error.get("message") {
                    message = m.clone();
                }
                if let Some(Value::String(t)) = error.get("type") {
                    error_type = t.clone();
                }
                if let Some(Value::String(c)) = error.get("code") {
                    code = Some(c.clone());
                }
            } else if let Some(Value::String(m)) = payload.get("message") {
                message = m.clone();
            } else if let Some(Value::String(e)) = payload.get("error") {
                message = e.clone();
            }
        } else if !text.is_empty() {
            message = text.clone();
            body = Some(Value::String(text));
        }

        CherryUpstreamError {
            message,
            status_code,
            error_type,
            code,
            body,
            content_type,
        }
    }
}
