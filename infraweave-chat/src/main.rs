use anyhow::{Context, Result};
use axum::{routing::get, routing::post, Router};
use infraweave_chat::{handler, llm::LlmClient};
use std::sync::Arc;
use tower_http::{cors::CorsLayer, trace::TraceLayer};
use tracing::info;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "infraweave_chat=info,tower_http=info".into()),
        )
        .init();

    let api_endpoint = std::env::var("INFRAWEAVE_API_ENDPOINT")
        .context("INFRAWEAVE_API_ENDPOINT is required (e.g. https://api.infraweave.example.com)")?;
    let port: u16 = std::env::var("PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(8090);

    let llm = build_llm().await?;
    let tools = Arc::new(infraweave_tools::registry());

    let state = handler::AppState {
        llm,
        tools,
        api_endpoint,
    };

    let app = Router::new()
        .route("/health", get(|| async { "ok" }))
        .route("/chat", post(handler::chat))
        .route("/events", post(handler::non_http_event))
        .fallback(handler::not_found)
        .layer(TraceLayer::new_for_http())
        .layer(CorsLayer::permissive())
        .with_state(state);

    let addr = format!("0.0.0.0:{port}");
    info!("infraweave-chat listening on {addr}");
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

#[cfg(feature = "bedrock")]
async fn build_llm() -> Result<Arc<dyn LlmClient>> {
    let model_id =
        std::env::var("BEDROCK_MODEL_ID").unwrap_or_else(|_| "us.amazon.nova-pro-v1:0".to_string());
    info!("using Bedrock model `{model_id}`");
    let client = infraweave_chat::llm::bedrock::BedrockClient::from_env(model_id).await;
    Ok(Arc::new(client))
}

#[cfg(not(feature = "bedrock"))]
async fn build_llm() -> Result<Arc<dyn LlmClient>> {
    anyhow::bail!("no LLM backend feature is enabled; rebuild with --features bedrock")
}
