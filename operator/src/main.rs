use env_common::interface::{initialize_project_id_and_region, GenericCloudHandler};
use log::info;

mod apply;
mod defs;
mod logging;
mod operator;

use logging::setup_logging;
use operator::start_operator;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    setup_logging().expect("Failed to initialize logging.");
    initialize_project_id_and_region().await;

    info!("This message will be logged to both stdout and the file.");

    let handler = GenericCloudHandler::default().await;
    start_operator(&handler).await?;

    Ok(())
}
