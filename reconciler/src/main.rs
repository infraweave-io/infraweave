use env_common::interface::{initialize_project_id_and_region, GenericCloudHandler};
use env_common::logic::driftcheck_infra;
use env_common::telemetry;
use env_defs::{CloudProvider, ExtraData};
use futures::future::join_all;
use lambda_runtime::{service_fn, Error, LambdaEvent};
use log::{error, info};
use serde_json::{json, Value};
use tracing::Instrument;

// The service-entry span is the one main wraps this in, which carries otel.kind
// and the adopted trace context; this is its child.
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

    let fun = service_fn(|event| async move {
        // Service-entry span for this run, joined to whatever trace the runtime
        // propagated; entry_span resolves that per platform so nothing here is
        // tied to one cloud.
        let response = func(event).instrument(telemetry::entry_span()).await;

        // Export this run's spans before the execution environment is frozen or
        // torn down. shutdown_tracing() below only runs when the serving loop
        // exits, which for a hosted function is effectively never, so without
        // this the spans are lost.
        //
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
