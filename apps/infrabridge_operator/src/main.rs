use log::info;

mod apply;
mod crd;
mod defs;
mod finalizer;
mod kind;
mod logging;
mod module;
mod operator;
mod other;
mod patch;
mod status;

use logging::setup_logging;
use operator::start_operator;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    setup_logging().expect("Failed to initialize logging.");

    info!("This message will be logged to both stdout and the file.");

    start_operator().await?;

    Ok(())
}
