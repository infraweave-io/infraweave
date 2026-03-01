use env_common::interface::initialize_project_id_and_region;
use internal_api::http_router;
#[cfg(feature = "local")]
use internal_api::local_setup;
use std::net::SocketAddr;
use tokio::net::TcpListener;
use tower_http::trace::TraceLayer;

#[tokio::main]
async fn main() -> std::io::Result<()> {
    env_logger::init();

    #[cfg(feature = "local")]
    let _infra = match local_setup::start_local_infrastructure().await {
        Ok(v) => Some(v),
        Err(e) => {
            eprintln!("Error starting local infrastructure: {:?}", e);
            return Err(std::io::Error::new(std::io::ErrorKind::Other, e));
        }
    };

    initialize_project_id_and_region().await;

    let port = std::env::var("PORT")
        .unwrap_or_else(|_| "8080".to_string())
        .parse::<u16>()
        .expect("Invalid port number");

    let app = http_router::create_router().layer(TraceLayer::new_for_http());

    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    let listener = TcpListener::bind(&addr).await?;

    log::info!("Starting local HTTP server on http://0.0.0.0:{}", port);
    println!("Server running at http://0.0.0.0:{}", port);
    println!("\nExample requests:");
    println!("  curl http://127.0.0.1:{}/api/v1/modules", port);
    println!("  curl http://127.0.0.1:{}/api/v1/projects", port);

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
}

async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }

    println!("\nShutting down gracefully... (stopping containers)");
}
