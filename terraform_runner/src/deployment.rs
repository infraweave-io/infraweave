use anyhow::{anyhow, Result};
use env_common::interface::GenericCloudHandler;
use env_defs::ApiInfraPayload;
use env_defs::{CloudProvider, DeploymentResp};

pub async fn get_initial_deployment(
    payload: &ApiInfraPayload,
    handler: &GenericCloudHandler,
) -> Result<Option<DeploymentResp>> {
    let deployment_id = &payload.deployment_id;
    let environment = &payload.environment;

    match handler
        .get_deployment(deployment_id, environment, false)
        .await
    {
        Ok(deployment) => match deployment {
            Some(deployment) => {
                log::info!("Deployment found: {:?}", deployment);
                Ok(Some(deployment))
            }
            None => {
                log::info!("Deployment not found");
                Ok(None)
            }
        },
        Err(e) => {
            log::info!("Error getting initial deployment: {}", e);
            Err(anyhow!("Error getting initial deployment: {}", e))
        }
    }
}
