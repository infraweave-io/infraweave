mod read;

use anyhow::Result;
use env_common::interface::{initialize_project_id_and_region, GenericCloudHandler};
use env_utils::otel_tracing;
use terraform_runner::{run_terraform_runner, setup_misc};
use tracing::Instrument;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    otel_tracing::init_tracing("terraform-runner").expect("Failed to initialize tracing");

    // Wrap the whole runner in a span carrying the upstream trace id so
    // every log line / OTel span is tagged with it. The API sets TRACE_ID
    // on the ECS container override.
    let trace_id = std::env::var("TRACE_ID").unwrap_or_default();
    let root_span = tracing::info_span!("terraform_runner", trace_id = %trace_id);

    let result = async {
        initialize_project_id_and_region().await;
        let handler = GenericCloudHandler::default().await;
        setup_misc().await;
        run_terraform_runner(&handler).await
    }
    .instrument(root_span)
    .await;

    otel_tracing::shutdown_tracing();
    result?;
    Ok(())
}
