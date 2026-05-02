use serde_json::{json, Value};

pub fn to_cherry_payload(request_payload: &Value, upstream_model: &str) -> Result<Value, String> {
    let messages = request_payload.get("messages");
    match messages {
        Some(Value::Array(arr)) if !arr.is_empty() => {}
        _ => return Err("'messages' must be a non-empty array.".to_string()),
    }

    let mut payload = request_payload.clone();
    if let Some(obj) = payload.as_object_mut() {
        obj.insert("model".to_string(), json!(upstream_model));
    }
    Ok(payload)
}

pub fn format_stream_line_as_sse(line: &str) -> Option<String> {
    let raw = line.trim_matches(|c| c == '\r' || c == '\n');
    if raw.is_empty() {
        return None;
    }

    if raw.starts_with(':')
        || raw.starts_with("event:")
        || raw.starts_with("id:")
        || raw.starts_with("retry:")
        || raw.starts_with("data:")
    {
        if raw == "data: [DONE]" {
            return Some("data: [DONE]\n\n".to_string());
        }
        if raw.starts_with("data:") {
            return Some(format!("{}\n\n", raw));
        }
        return Some(format!("{}\n", raw));
    }

    if raw == "[DONE]" {
        return Some("data: [DONE]\n\n".to_string());
    }

    Some(format!("data: {}\n\n", raw))
}
