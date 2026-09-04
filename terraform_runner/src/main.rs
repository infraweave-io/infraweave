mod read;

use anyhow::Result;
use env_common::interface::{initialize_project_id_and_region, GenericCloudHandler};
use env_common::telemetry;
use terraform_runner::{run_terraform_runner, setup_misc};
use tracing::Instrument;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    telemetry::init_tracing("terraform-runner")
        .await
        .expect("Failed to initialize tracing");

    // Wrap the whole runner in a root span. If the launching service
    // (internal-api / reconciler) propagated its trace context via the TRACE_ID
    // container override, adopt it as the parent so the runner's spans join the
    // caller's trace instead of starting a disconnected one.
    //
    // `otel.kind = "server"` makes the X-Ray exporter emit this as a top-level
    // *segment* (a service entry point), so the runner stays visible as its own
    // node under the caller's trace rather than becoming an orphaned subsegment.
    // With nothing propagated it is simply a root.
    // Declared empty and filled in once the payload is parsed, so the root span
    // can be filtered on without drilling into children.
    let trace_id = std::env::var("TRACE_ID").unwrap_or_default();
    let root_span = tracing::info_span!(
        "terraform_runner",
        otel.kind = "server",
        infraweave.job_id = tracing::field::Empty,
        // Namespaced because `deployment.environment` is an OTel resource
        // attribute naming the AWS account, while this one is InfraWeave's own
        // environment id. Two unrelated meanings that otherwise read alike on
        // the same span.
        infraweave.deployment_id = tracing::field::Empty,
        infraweave.environment_id = tracing::field::Empty,
        infraweave.project_id = tracing::field::Empty,
        region = tracing::field::Empty,
        module = tracing::field::Empty,
        version = tracing::field::Empty,
        command = tracing::field::Empty,
        initiated_by = tracing::field::Empty,
    );
    env_utils::otel_tracing::set_span_parent_from_traceparent(&root_span, &trace_id);

    let result = async {
        initialize_project_id_and_region().await;
        let handler = GenericCloudHandler::default().await;
        setup_misc().await;
        run_terraform_runner(&handler).await
    }
    .instrument(root_span)
    .await;

    // The root span above closed when its future was dropped at the end of that
    // statement, so this flush is what actually exports it. Off the async worker
    // for the same reason as in the Lambda handlers: shutdown blocks until the
    // batch processor drains, and that processor runs on this runtime.
    let _ = tokio::task::spawn_blocking(telemetry::shutdown_tracing).await;

    if let Some(drain) = collector_drain_wait() {
        tokio::time::sleep(drain).await;
    }

    result?;
    Ok(())
}

/// How long to stay alive after flushing, for a collector sidecar to drain.
///
/// With one in front of `otlp-http`, the ECS task tears down the moment this
/// (essential) container exits — which kills the collector before it can export
/// the root `terraform_runner` segment that just closed and flushed. Without
/// that root segment X-Ray drops the whole trace as orphaned subsegments.
///
/// No image bundles a collector, so the default is not to wait; a deployment
/// that adds a sidecar sets `TELEMETRY_DRAIN_SECONDS`. Exporting in-process
/// needs no wait at all — `shutdown_tracing` has already drained.
fn collector_drain_wait() -> Option<std::time::Duration> {
    let raw = std::env::var("TELEMETRY_DRAIN_SECONDS").ok()?;
    match raw.trim().parse::<u64>() {
        Ok(0) => None,
        Ok(seconds) => Some(std::time::Duration::from_secs(seconds)),
        Err(e) => {
            eprintln!("Ignoring TELEMETRY_DRAIN_SECONDS={raw:?}: {e}");
            None
        }
    }
}
