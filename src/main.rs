use std::net::SocketAddr;
use std::sync::Arc;

use axum::{
    middleware,
    routing::{get, post},
    Extension, Router,
};
use tower_http::trace::TraceLayer;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

mod auth;
mod config;
mod mappers;
mod routes;
mod signatures;
mod upstream;

use config::Settings;
use routes::{chat_completions, health, index, list_models, AppState};
use upstream::CherryClient;

#[tokio::main]
async fn main() {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    dotenvy::dotenv().ok();

    let settings = Settings::from_env();
    let cherry_client = CherryClient::new(settings.clone());

    let state = Arc::new(AppState {
        settings: settings.clone(),
        cherry_client,
    });

    let app = Router::new()
        .route("/", get(index))
        .route("/health", get(health))
        .route("/v1/models", get(list_models))
        .route("/v1/chat/completions", post(chat_completions))
        .layer(middleware::from_fn(auth::auth_middleware))
        .layer(TraceLayer::new_for_http())
        .layer(Extension(settings.clone()))
        .with_state(state);

    let addr: SocketAddr = format!("{}:{}", settings.host, settings.port)
        .parse()
        .expect("Invalid host or port");

    tracing::info!("Server listening on http://{}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
