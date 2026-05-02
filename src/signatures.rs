use hmac::{Hmac, Mac};
use serde_json::Value;
use sha2::Sha256;
use std::time::{SystemTime, UNIX_EPOCH};

const CHERRY_CLIENT_ID: &str = "cherry-studio";
const CHERRY_SIGNING_SECRET: &str = "K3RNPFx19hPh1AHr5E1wBEFfi4uYUjoCFuzjDzvS9cAWD8KuKJR8FOClwUpGqRRX.GvI6I5ZrEHcGOWjO5AKhJKGmnwwGfM62XKpWqkjhvzRU2NZIinM77aTGIqhqys0g";

type HmacSha256 = Hmac<Sha256>;

fn normalize_body(body: Option<&Value>) -> Option<Value> {
    body.cloned()
}

pub fn serialize_body(body: Option<&Value>) -> String {
    match normalize_body(body) {
        None => String::new(),
        Some(Value::String(s)) => s,
        Some(other) => serde_json::to_string(&other).unwrap_or_default(),
    }
}

pub fn generate_signature_headers(
    method: &str,
    path: &str,
    query: &str,
    body: Option<&Value>,
    timestamp: Option<u64>,
) -> Vec<(&'static str, String)> {
    let resolved_timestamp = timestamp
        .map(|t| t.to_string())
        .unwrap_or_else(|| {
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs()
                .to_string()
        });

    let body_string = serialize_body(body);
    let raw = format!(
        "{}\n{}\n{}\n{}\n{}\n{}",
        method.to_uppercase(),
        path,
        query,
        CHERRY_CLIENT_ID,
        resolved_timestamp,
        body_string
    );

    let mut mac = HmacSha256::new_from_slice(CHERRY_SIGNING_SECRET.as_bytes())
        .expect("HMAC can take key of any size");
    mac.update(raw.as_bytes());
    let signature = hex::encode(mac.finalize().into_bytes());

    vec![
        ("X-Client-ID", CHERRY_CLIENT_ID.to_string()),
        ("X-Timestamp", resolved_timestamp),
        ("X-Signature", signature),
    ]
}
