use aws_sdk_lambda::primitives::Blob;
use aws_sdk_lambda::types::InvocationType;
use aws_sdk_lambda::Client;
use env_defs::DeploymentResp;
use log::{debug, error, warn};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Serialize, Deserialize)]
struct ApiResourcesPayload {
    resource_groups_name: String,
    format: String,
}

pub async fn list_deployments(region: &str) -> anyhow::Result<Vec<DeploymentResp>> {
    let deployments = get_deployments(region).await?;
    print_api_resources(deployments.clone());
    Ok(deployments)
}

pub async fn describe_deployment_id(
    deployment_id: &str,
    region: &str,
) -> Result<DeploymentResp, anyhow::Error> {
    // Naive version, will not scale well. TODO: add functionality in lambda to filter by deployment_id
    let deployments = get_deployments(region).await?;
    if let Some(deployment) = deployments
        .into_iter()
        .find(|d| d.deployment_id == deployment_id)
    {
        println!("Describing deployment id:");
        print_api_resources(vec![deployment.clone()]);
        return Ok(deployment);
    } else {
        Err(anyhow::anyhow!(
            "Deployment {} not found in region {}",
            deployment_id,
            region,
        ))
    }
}

async fn get_deployments(region: &str) -> anyhow::Result<Vec<DeploymentResp>> {
    let environment = "dev";
    let payload = ApiResourcesPayload {
        resource_groups_name: format!("resources-all-dev-{}-{}", region, environment),
        format: "json".to_string(),
    };

    let shared_config = aws_config::from_env().load().await;
    let region_name = shared_config.region().unwrap();

    let client = Client::new(&shared_config);
    let api_function_name = "resourceGathererFunction";

    let serialized_payload = serde_json::to_vec(&payload).unwrap();
    let payload_blob = Blob::new(serialized_payload);

    warn!(
        "Invoking job in region {} using {} with payload: {:?}",
        region_name, api_function_name, payload
    );

    let request = client
        .invoke()
        .function_name(api_function_name)
        .invocation_type(InvocationType::RequestResponse)
        .payload(payload_blob);

    let response = match request.send().await {
        Ok(response) => response,
        Err(e) => {
            error!("Failed to invoke Lambda: {}", e);
            let error_message = format!("Failed to invoke Lambda: {}", e);
            return Err(anyhow::anyhow!(error_message));
        }
    };

    if let Some(blob) = response.payload {
        let bytes = blob.into_inner();
        let response_string = String::from_utf8(bytes).expect("response not valid UTF-8");
        warn!("Lambda response: {:?}", response_string);

        // Decode the outer JSON layer to get the actual JSON string
        let inner_json_str: String =
            serde_json::from_str(&response_string).expect("response not valid JSON string");

        debug!("Inner JSON string: {:?}", inner_json_str);
        let parsed_json: Value =
            serde_json::from_str(&inner_json_str).expect("inner response not valid JSON");

        debug!("Parsed JSON: {:?}", parsed_json);

        if let Some(deployments) = parsed_json.as_array() {
            let mut deployments_vec: Vec<DeploymentResp> = vec![];
            for deployment in deployments {
                warn!("Deployment: {:?}", deployment);
                let val: DeploymentResp = DeploymentResp {
                    cloud_id: deployment
                        .get("resource_arn")
                        .expect("resource_arn not found")
                        .as_str()
                        .expect("resource_arn not a string")
                        .to_string(),
                    cloud_type: deployment
                        .get("resource_type")
                        .expect("resource_type not found")
                        .as_str()
                        .expect("resource_type not a string")
                        .to_string(),
                    deployment_id: deployment
                        .get("deployment_id")
                        .expect("deployment_id not found")
                        .as_str()
                        .expect("deployment_id not a string")
                        .to_string(),
                    // name: deployment
                    //     .get("name")
                    //     .expect("name not found")
                    //     .as_str()
                    //     .expect("name not a string")
                    //     .to_string(),
                    // environment: deployment
                    //     .get("environment")
                    //     .expect("environment not found")
                    //     .as_str()
                    //     .expect("environment not a string")
                    //     .to_string(),
                    // module: deployment
                    //     .get("module")
                    //     .expect("module not found")
                    //     .as_str()
                    //     .expect("module not a string")
                    //     .to_string(),
                    // last_activity_epoch: deployment
                    //     .get("last_activity_epoch")
                    //     .expect("last_activity_epoch not found")
                    //     .as_i64()
                    //     .expect("last_activity_epoch not an integer"),
                };

                deployments_vec.push(val.clone());
                warn!("Parsed Deployment: {:?}", val);
            }
            return Ok(deployments_vec);
        } else {
            panic!("Expected an array of deployments");
        }
        Ok(vec![])
    } else {
        Err(anyhow::anyhow!("Payload missing from Lambda response"))
    }
}

fn print_api_resources(deployments: Vec<DeploymentResp>) {
    println!(
        "{:<35} {:<60} {:<20}",
        "DeploymentId", "ARN:", "ResourceType"
    );
    for deployment in &deployments {
        println!(
            "{:<35} {:<60} {:<20}",
            deployment.deployment_id, deployment.cloud_id, deployment.cloud_type,
        );
    }
}
