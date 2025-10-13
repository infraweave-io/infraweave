use env_common::interface::{initialize_project_id_and_region, GenericCloudHandler};
use log::info;
use std::env;

mod apply;
mod defs;
mod logging;
mod operator;
mod validation;
mod webhook;

use logging::setup_logging;
use operator::start_operator;
use webhook::start_webhook_server;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize rustls crypto provider
    let _ = rustls::crypto::ring::default_provider().install_default();

    setup_logging().expect("Failed to initialize logging.");

    let mode = env::var("MODE").unwrap_or_else(|_| "operator".to_string());

    match mode.to_lowercase().as_str() {
        "webhook" => {
            info!("Starting in WEBHOOK mode");
            run_webhook_mode().await?;
        }
        "operator" => {
            info!("Starting in OPERATOR mode");
            run_operator_mode().await?;
        }
        _ => {
            return Err(format!("Invalid MODE '{}'. Must be 'operator' or 'webhook'", mode).into());
        }
    }

    Ok(())
}

async fn run_operator_mode() -> Result<(), Box<dyn std::error::Error>> {
    initialize_project_id_and_region().await;

    let handler = GenericCloudHandler::default().await;
    start_operator(&handler).await?;

    Ok(())
}

async fn run_webhook_mode() -> Result<(), Box<dyn std::error::Error>> {
    let webhook_port = env::var("WEBHOOK_PORT")
        .unwrap_or_else(|_| "8443".to_string())
        .parse::<u16>()
        .unwrap_or(8443);

    let handler = GenericCloudHandler::default().await;

    info!("Starting admission webhook server on port {}", webhook_port);
    start_webhook_server(handler, webhook_port).await?;

    Ok(())
}
