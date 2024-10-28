use env_common::interface::initialize_project_id;
use log::info;

mod apply;
mod crd;
mod defs;
mod finalizer;
mod kind;
mod logging;
mod operator;
mod patch;
mod status;
mod utils;

use logging::setup_logging;
use operator::start_operator;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    setup_logging().expect("Failed to initialize logging.");
    initialize_project_id().await;

    info!("This message will be logged to both stdout and the file.");

    start_operator().await?;

    Ok(())
}
