use env_common::interface::{initialize_project_id_and_region, GenericCloudHandler};
use env_common::logic::driftcheck_infra;
use env_defs::{CloudProvider, ExtraData};
use futures::future::join_all;
use lambda_runtime::{service_fn, Error, LambdaEvent};
use log::{error, info};
use serde_json::{json, Value};

async fn func(event: LambdaEvent<Value>) -> Result<Value, Error> {
    let (_event, _context) = event.into_parts();

    let handler = GenericCloudHandler::default().await;
    let deployments = match handler.get_deployments_to_driftcheck().await {
        Ok(deployments) => {
            info!("Deployments to check for drift: {:?}", deployments);
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
        async move {
            println!(
                "Deploymentid: {}, environment: {}",
                deployment_id, environment
            );
            let remediate = deployment.drift_detection.auto_remediate;
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
    initialize_project_id_and_region().await;

    let fun = service_fn(func);
    lambda_runtime::run(fun).await?;

    Ok(())
}
