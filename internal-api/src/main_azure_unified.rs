// Azure Functions Custom Handler - Simple HTTP server for Azure Functions
use env_common::interface::initialize_project_id_and_region;
use internal_api::http_router;
use internal_api::otel_tracing;
use std::net::SocketAddr;
use tokio::net::TcpListener;
use tower_http::trace::TraceLayer;

use axum::extract::Request;
use axum::middleware::{self, Next};
use axum::response::Response;

async fn normalize_azure_headers(mut req: Request, next: Next) -> Response {
    let headers = req.headers_mut();

    // 1. SECURITY: Remove any client-supplied 'x-auth-user' to prevent spoofing
    headers.remove("x-auth-user");

    // 2. Extract Azure App Service Authentication Header
    // 'X-MS-CLIENT-PRINCIPAL-NAME' typically contains the user's email/login
    let azure_user = headers
        .get("X-MS-CLIENT-PRINCIPAL-NAME")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    if let Some(user) = azure_user {
        log::info!("Authenticated Azure user: {}", user);
        // 3. Inject as our standard internal header
        if let Ok(val) = axum::http::HeaderValue::from_str(&user) {
            headers.insert("x-auth-user", val);
        }
    }

    next.run(req).await
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    if let Err(e) = otel_tracing::init_tracing("internal-api-azure") {
        eprintln!("Failed to initialize OpenTelemetry: {}", e);
        env_logger::init();
    }
    initialize_project_id_and_region().await;

    // Azure Functions forwards requests to the custom handler on this port
    let port = std::env::var("FUNCTIONS_CUSTOMHANDLER_PORT")
        .or_else(|_| std::env::var("PORT"))
        .unwrap_or_else(|_| "8080".to_string())
        .parse::<u16>()
        .expect("Invalid port number");

    let app = http_router::create_router()
        .layer(middleware::from_fn(normalize_azure_headers)) // Add normalization middleware
        .layer(TraceLayer::new_for_http());

    // Bind to 0.0.0.0 for Azure Functions (not 127.0.0.1)
    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    let listener = TcpListener::bind(&addr).await?;

    log::info!(
        "Starting Azure Functions custom handler on 0.0.0.0:{}",
        port
    );
    println!("Azure Functions custom handler running on port {}", port);

    axum::serve(listener, app).await
}
