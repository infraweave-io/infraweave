use std::collections::HashMap;

use aws_sdk_lambda::primitives::Blob;
use aws_sdk_lambda::types::InvocationType;
use aws_sdk_lambda::Client;
use env_defs::DeploymentResp;
use log::{debug, error, warn};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Serialize, Deserialize)]
struct ApiDeploymentsPayload {
    deployment_id: String,
}

pub async fn list_deployments() -> anyhow::Result<Vec<DeploymentResp>> {
    let deployments = get_all_deployments().await?;
    print_api_resources(deployments.clone());
    Ok(deployments)
}

pub async fn describe_deployment_id(
    deployment_id: &str,
    region: &str,
) -> Result<DeploymentResp, anyhow::Error> {
    // Naive version, will not scale well. TODO: add functionality in lambda to filter by deployment_id
    let deployments = get_all_deployments().await?;
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

async fn get_all_deployments() -> anyhow::Result<Vec<DeploymentResp>> {
    let environment = "dev";
    let payload = ApiDeploymentsPayload {
        deployment_id: "".to_string(),
    };

    let shared_config = aws_config::from_env().load().await;
    let region_name = shared_config.region().unwrap();

    let client = Client::new(&shared_config);
    let api_function_name = "deploymentStatusApi";

    let serialized_payload = serde_json::to_vec(&payload).unwrap();
    let payload_blob = Blob::new(serialized_payload);

    warn!(
        "Invoking job in region {} using {} with payload: {:?}",
        region_name, api_function_name, payload
    );

    let request = client
        .invoke()
        .function_name(api_function_name)
        .invocation_type(InvocationType::RequestResponse);
    // .payload(payload_blob);

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
                    epoch: deployment
                        .get("epoch")
                        .expect("epoch not found")
                        .as_f64()
                        .expect("epoch not a decimal")
                        .round() as i64,
                    deployment_id: deployment
                        .get("deployment_id")
                        .expect("deployment_id not found")
                        .as_str()
                        .expect("deployment_id not a string")
                        .to_string(),
                    module: deployment
                        .get("module")
                        .expect("module not found")
                        .as_str()
                        .expect("module not a string")
                        .to_string(),
                    environment: deployment
                        .get("environment")
                        .expect("environment not found")
                        .as_str()
                        .expect("environment not a string")
                        .to_string(),
                    inputs: deployment
                        .get("input_variables")
                        .expect("inputs not found")
                        .as_object() // Expect this to be a JSON object
                        .expect("inputs not a JSON object")
                        .iter()
                        .map(|(key, value)| (key.clone(), value.as_str().unwrap_or("").to_string())) // Extract and convert each entry
                        .collect::<HashMap<String, String>>(),
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
        "{:<35} {:<25} {:<15} {:<20} {:<20}",
        "DeploymentId", "Module", "Environment", "Time", "Inputs"
    );
    for deployment in &deployments {
        println!(
            "{:<35} {:<25} {:<15} {:<20} {:<20}",
            deployment.deployment_id,
            deployment.module,
            deployment.environment,
            deployment.epoch,
            serde_json::to_string(&deployment.inputs).unwrap()
        );
    }
}
