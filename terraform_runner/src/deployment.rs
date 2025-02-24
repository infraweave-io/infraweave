use std::process::exit;

use env_common::interface::GenericCloudHandler;
use env_defs::ApiInfraPayload;
use env_defs::{CloudProvider, DeploymentResp};

pub async fn get_initial_deployment(
    payload: &ApiInfraPayload,
    handler: &GenericCloudHandler,
) -> Option<DeploymentResp> {
    let deployment_id = &payload.deployment_id;
    let environment = &payload.environment;

    match handler
        .get_deployment(deployment_id, environment, false)
        .await
    {
        Ok(deployment) => match deployment {
            Some(deployment) => {
                println!("Deployment found: {:?}", deployment);
                Some(deployment)
            }
            None => {
                println!("Deployment not found");
                None
            }
        },
        Err(e) => {
            println!("Error getting deployment and dependents: {}", e);
            exit(1);
        }
    }
}
