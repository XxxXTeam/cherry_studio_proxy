use std::collections::HashMap;
use std::env;

const DEFAULT_USER_AGENT: &str = concat!(
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 ",
    "(KHTML, like Gecko) CherryStudio/1.9.4 Chrome/146.0.7680.188 ",
    "Electron/41.2.1 Safari/537.36"
);

#[derive(Clone, Debug)]
pub struct Settings {
    pub host: String,
    pub port: u16,
    pub openai_api_key: Option<String>,
    pub cherry_base_url: String,
    pub cherry_models: HashMap<String, String>,
    pub cherry_user_agent: String,
    pub cherry_referer: String,
    pub cherry_title: String,
    pub request_timeout: f64,
    pub log_sse_stream: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            host: "0.0.0.0".to_string(),
            port: 8000,
            openai_api_key: None,
            cherry_base_url: "https://api.cherry-ai.com".to_string(),
            cherry_models: [("qwen".to_string(), "qwen".to_string())]
                .into_iter()
                .collect(),
            cherry_user_agent: DEFAULT_USER_AGENT.to_string(),
            cherry_referer: "https://cherry-ai.com".to_string(),
            cherry_title: "Cherry Studio".to_string(),
            request_timeout: 60.0,
            log_sse_stream: false,
        }
    }
}

impl Settings {
    pub fn from_env() -> Self {
        let mut settings = Self::default();

        if let Ok(host) = env::var("HOST") {
            settings.host = host;
        }
        if let Ok(port) = env::var("PORT") {
            if let Ok(port) = port.parse() {
                settings.port = port;
            }
        }
        if let Ok(key) = env::var("OPENAI_API_KEY") {
            if !key.trim().is_empty() {
                settings.openai_api_key = Some(key);
            }
        }
        if let Ok(url) = env::var("CHERRY_BASE_URL") {
            settings.cherry_base_url = url.trim_end_matches('/').to_string();
        }

        let fallback_model = env::var("CHERRY_MODEL").unwrap_or_else(|_| "qwen".to_string());
        if let Ok(models) = env::var("CHERRY_MODELS") {
            settings.cherry_models = parse_model_aliases(&models, &fallback_model);
        }

        if let Ok(ua) = env::var("CHERRY_USER_AGENT") {
            settings.cherry_user_agent = ua;
        }
        if let Ok(referer) = env::var("CHERRY_REFERER") {
            settings.cherry_referer = referer;
        }
        if let Ok(title) = env::var("CHERRY_TITLE") {
            settings.cherry_title = title;
        }
        if let Ok(timeout) = env::var("REQUEST_TIMEOUT") {
            if let Ok(timeout) = timeout.parse() {
                settings.request_timeout = timeout;
            }
        }
        if let Ok(log) = env::var("LOG_SSE_STREAM") {
            settings.log_sse_stream = is_truthy(&log);
        }

        settings
    }

    pub fn chat_completions_path(&self) -> &str {
        "/chat/completions"
    }

    pub fn model_owner(&self) -> &str {
        "cherry-proxy"
    }

    pub fn default_public_model(&self) -> String {
        self.cherry_models.keys().next().cloned().unwrap_or_else(|| "qwen".to_string())
    }

    pub fn resolve_upstream_model(&self, public_model: &str) -> Option<String> {
        self.cherry_models.get(public_model).cloned()
    }
}

fn parse_model_aliases(raw: &str, fallback: &str) -> HashMap<String, String> {
    let raw = raw.trim();
    if raw.is_empty() {
        let mut map = HashMap::new();
        map.insert(fallback.to_string(), fallback.to_string());
        return map;
    }

    let mut aliases = HashMap::new();
    let normalized = raw.replace('\r', "\n").replace([',', ';'], "\n");
    for entry in normalized.split('\n') {
        let item = entry.trim();
        if item.is_empty() {
            continue;
        }

        let (public, upstream) = if let Some(pos) = item.find('=') {
            let public = item[..pos].trim();
            let upstream = item[pos + 1..].trim();
            (public, upstream)
        } else {
            (item, item)
        };

        if !public.is_empty() && !upstream.is_empty() {
            aliases.insert(public.to_string(), upstream.to_string());
        }
    }

    if aliases.is_empty() {
        aliases.insert(fallback.to_string(), fallback.to_string());
    }
    aliases
}

fn is_truthy(value: &str) -> bool {
    matches!(value.trim().to_lowercase().as_str(), "1" | "true" | "yes" | "on")
}
