mod read;

use anyhow::Result;
use env_common::interface::{initialize_project_id_and_region, GenericCloudHandler};
use env_utils::setup_logging;
use terraform_runner::{run_terraform_runner, setup_misc};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    setup_logging().expect("Failed to initialize logging.");
    initialize_project_id_and_region().await;

    let handler = GenericCloudHandler::default().await;
    setup_misc().await;

    run_terraform_runner(&handler).await?;
    Ok(())
}
