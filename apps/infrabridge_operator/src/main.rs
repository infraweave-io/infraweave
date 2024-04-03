use log::info;

mod patch;
mod module;
mod apply;
mod status;
mod operator;
mod other;

use other::setup_logging;
use operator::start_operator;

const FINALIZER_NAME: &str = "deletion-handler.finalizer.infrabridge.io";

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    setup_logging().expect("Failed to initialize logging.");

    info!("This message will be logged to both stdout and the file.");

    start_operator().await?;

    Ok(())
}
