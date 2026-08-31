use env_common::interface::{initialize_project_id_and_region, GenericCloudHandler};
use env_common::logic::driftcheck_infra;
use env_common::telemetry;
use env_defs::{CloudProvider, ExtraData};
use futures::future::join_all;
use lambda_runtime::{service_fn, Error, LambdaEvent};
use log::{error, info};
use serde_json::{json, Value};
use tracing::Instrument;

// The service-entry span is `lambda_invocation` in main, which carries
// otel.kind and the adopted Lambda trace context; this is its child.
#[tracing::instrument(skip(event))]
async fn func(event: LambdaEvent<Value>) -> Result<Value, Error> {
    let (_event, _context) = event.into_parts();

    let handler = GenericCloudHandler::default().await;
    let deployments = match handler.get_deployments_to_driftcheck().await {
        Ok(deployments) => {
            info!("Deployments to check for drift: {:?}", deployments);
            println!("Deployments to check for drift:");
            deployments.iter().for_each(|d| {
                println!("{}: {}", d.deployment_id, d.environment);
            });
            deployments
        }
        Err(e) => {
            error!("Failed to get deployments to check for drift: {}", e);
            vec![]
        }
    };

    // Launch drift checks for each deployment asynchronously to run them in parallel
    let drift_checks = deployments.clone().into_iter().map(|deployment| {
        let deployment_id = deployment.deployment_id.clone();
        let environment = deployment.environment.clone();

        // One span per deployment rather than one for the whole invocation.
        // Drift checks run concurrently, so without this their work interleaves
        // under a single span and a trace search by deployment cannot tell which
        // of them failed. These are the same correlation keys internal-api and
        // the runner use, so one deployment's history spans all three services.
        let span = tracing::info_span!(
            "driftcheck",
            infraweave.deployment_id = %deployment_id,
            infraweave.environment_id = %environment,
        );

        async move {
            let remediate = deployment.drift_detection.auto_remediate;
            info!(
                "Starting scheduled drift check for {} in {} with auto_remediate={}",
                deployment_id, environment, remediate
            );
            let handler = GenericCloudHandler::default().await;
            match driftcheck_infra(
                &handler,
                &deployment_id,
                &environment,
                remediate,
                ExtraData::None,
            )
            .await
            {
                Ok(_) => {
                    info!("Successfully requested drift check");
                }
                Err(e) => {
                    error!("Failed to request drift check: {}", e);
                }
            }
        }
        .instrument(span)
    });

    join_all(drift_checks).await;

    let drift_checked_deployment_ids = deployments
        .into_iter()
        .map(|deployment| {
            json!({
                "deployment_id": deployment.deployment_id,
                "environment": deployment.environment,
                // "seconds_since_last_driftcheck": deployment.next_drift_check_epoch - get_epoch(),

            })
        })
        .collect::<Vec<Value>>();

    let response = json!({
        "status": "successful",
        "drift_checked_deployments": drift_checked_deployment_ids,
    });
    println!("{}", serde_json::to_string_pretty(&response).unwrap());
    Ok(response)
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    telemetry::init_tracing("reconciler")
        .await
        .expect("Failed to initialize tracing.");
    initialize_project_id_and_region().await;

    // Flush each invocation's spans before the execution environment freezes.
    // shutdown_tracing() below only runs when the runtime loop exits, which on
    // Lambda is effectively never, so without this the spans are lost.
    let fun = service_fn(|event| async move {
        // Root span for the invocation. `otel.kind = "server"` is load-bearing:
        // the X-Ray exporter turns Server/Consumer spans into *segments* and
        // everything else into *subsegments*, and a subsegment whose parent
        // segment X-Ray never received is orphaned and dropped without any
        // error. Without a server-kind root the reconciler's spans are silently
        // discarded on arrival.
        //
        // Joining Lambda's own trace matters even on a schedule-driven function:
        // its built-in active tracing records the invocation under a trace id of
        // its own, so without adopting it here every run produces two unrelated
        // traces. Same reasoning, and the same wrapping, as internal-api.
        let root = tracing::info_span!("lambda_invocation", otel.kind = "server");
        if let Ok(header) = std::env::var("_X_AMZN_TRACE_ID") {
            env_utils::otel_tracing::set_span_parent_from_xray_header(&root, &header);
        }

        let response = func(event).instrument(root).await;
        // Off the async worker: force_flush blocks until the batch processor
        // drains, and that processor runs on this same runtime, so calling it
        // inline deadlocks on a current-thread runtime and starves a worker on
        // a multi-threaded one.
        let _ = tokio::task::spawn_blocking(telemetry::force_flush_tracing).await;
        response
    });
    let result = lambda_runtime::run(fun).await;

    // Blocking, so off the async worker for the same reason as the
    // per-invocation flush above.
    let _ = tokio::task::spawn_blocking(telemetry::shutdown_tracing).await;
    result?;

    Ok(())
}
