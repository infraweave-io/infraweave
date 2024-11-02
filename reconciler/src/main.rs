use env_common::interface::{initialize_project_id, CloudHandler};
use env_common::logic::{driftcheck_infra, handler};
use log::{error, info};
use lambda_runtime::{service_fn, Error, LambdaEvent};
use serde_json::{json, Value};


async fn func(event: LambdaEvent<Value>) -> Result<Value, Error> {
    let (event, _context) = event.into_parts();
    let first_name = event["firstName"].as_str().unwrap_or("world");

    let deployments = match handler().get_deployments_to_driftcheck().await {
        Ok(deployments) => {
            info!("Deployments to check for drift: {:?}", deployments);
            deployments
        }
        Err(e) => {
            error!("Failed to get deployments to check for drift: {}", e);
            vec![]
        }
    };

    for deployment in deployments {
        println!(
            "Deploymentid: {}, environment: {}",
            deployment.deployment_id, deployment.environment
        );
        match driftcheck_infra(&deployment.deployment_id, &deployment.environment).await {
            Ok(_) => {
                info!("Successfully requested drift check");
                Ok(())
            }
            Err(e) => {
                Err(anyhow::anyhow!("Failed to request drift check: {}", e))
            }
        }.unwrap();
    }

    Ok(json!({ "message": format!("Hello, {}!", first_name) }))
}


#[tokio::main]
async fn main() -> Result<(), Error> {
    initialize_project_id().await;

    let fun = service_fn(func);
    lambda_runtime::run(fun).await?;

    Ok(())
}
