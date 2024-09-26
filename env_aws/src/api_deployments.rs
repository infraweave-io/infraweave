use env_defs::DeploymentResp;
use log::{error, warn};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::api::run_lambda;

#[derive(Debug, Serialize, Deserialize)]
pub struct ApiDeploymentsPayload {
    deployment_id: String,
    query: Value,
}

pub async fn list_deployments() -> anyhow::Result<Vec<DeploymentResp>> {
    let deployments = get_all_deployments().await?;
    print_api_resources(deployments.clone());
    Ok(deployments)
}

pub async fn describe_deployment_id(
    deployment_id: &str,
    environment: &str,
    region: &str,
) -> Result<DeploymentResp, anyhow::Error> {
    // Naive version, will not scale well. TODO: add functionality in lambda to filter by deployment_id

    // Probably works with
    // let response = read_db(serde_json::json!({...})).await?;
    // event = {
    //     'IndexName': 'DeploymentIdIndex',
    //     'KeyConditionExpression': 'deployment_id = :deployment_id',
    //     'ExpressionAttributeValues': {':deployment_id': deployment_id}
    // }

    let deployments = get_all_deployments().await?;
    if let Some(deployment) = deployments
        .into_iter()
        .find(|d| d.deployment_id == deployment_id && d.environment == environment)
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
    let response = read_db(serde_json::json!({
        "IndexName": "DeletedIndex",
        "KeyConditionExpression": "deleted = :deleted",
        "ExpressionAttributeValues": {":deleted": 0}
    }))
    .await?;

    let items = response.get("Items").expect("Items not found");

    if let Some(deployments) = items.as_array() {
        let mut deployments_vec: Vec<DeploymentResp> = vec![];
        for deployment in deployments {
            warn!("Deployment: {:?}", deployment);
            deployments_vec.push(map_to_deployment(deployment.clone()));
        }
        return Ok(deployments_vec);
    } else {
        panic!("Expected an array of deployments");
    }
}

fn map_to_deployment(value: Value) -> DeploymentResp {
    DeploymentResp {
        epoch: value
            .get("epoch")
            .expect("epoch not found")
            .as_f64()
            .expect("epoch is not a f64") as u128,
        deployment_id: value
            .get("deployment_id")
            .expect("deployment_id not found")
            .as_str()
            .expect("deployment_id not a string")
            .to_string(),
        status: value
            .get("status")
            .expect("status not found")
            .as_str()
            .expect("status not a string")
            .to_string(),
        module: value
            .get("module")
            .expect("module not found")
            .as_str()
            .expect("module not a string")
            .to_string(),
        module_version: value
            .get("module_version")
            .expect("module_version not found")
            .as_str()
            .expect("module_version not a string")
            .to_string(),
        environment: value
            .get("environment")
            .expect("environment not found")
            .as_str()
            .expect("environment not a string")
            .to_string(),
        variables: value.get("variables").expect("inputs not found").clone(),
        error_text: value
            .get("error_text")
            .unwrap_or(&Value::String("".to_string()))
            .as_str()
            .expect("error_text not a string")
            .to_string(),
        deleted: value
            .get("deleted")
            .expect("deleted not found")
            .as_f64()
            .expect("deleted not an f64")
            != 0.0,
    }
}

fn print_api_resources(deployments: Vec<DeploymentResp>) {
    println!(
        "{:<30} {:<20} {:<10} {:<17} {:<30} {:<20}",
        "DeploymentId", "Module", "Version", "Status", "Environment", "Last Update"
    );
    for deployment in &deployments {
        // Convert the epoch (in milliseconds) to a NaiveDateTime and then to UTC DateTime
        let naive =
            chrono::NaiveDateTime::from_timestamp_opt((deployment.epoch as i64) / 1000, 0).unwrap(); // Convert from ms to seconds
        let datetime_utc: chrono::DateTime<chrono::Utc> =
            chrono::DateTime::from_utc(naive, chrono::Utc);

        // Convert the UTC time to local time
        let datetime_local: chrono::DateTime<chrono::Local> =
            datetime_utc.with_timezone(&chrono::Local);

        println!(
            "{:<30} {:<20} {:<10} {:<17} {:<30} {:<20}",
            deployment.deployment_id,
            deployment.module,
            deployment.module_version,
            deployment.status,
            deployment.environment,
            datetime_local,
        );
    }
}

pub async fn set_deployment(deployment: DeploymentResp) -> anyhow::Result<String> {
    let mut payload = serde_json::json!({
        "event": "insert_db",
        "table": "deployments",
        "data": deployment
    });
    // hack: set deleted to int 0/1 since GSI does not support boolean in AWS: https://docs.aws.amazon.com/amazondynamodb/latest/APIReference/API_AttributeDefinition.html#DDB-Type-AttributeDefinition-AttributeType
    payload["data"]["deleted"] = serde_json::json!(if deployment.deleted { 1 } else { 0 });

    match run_lambda(payload).await {
        Ok(_) => Ok("".to_string()),
        Err(e) => {
            error!("Failed to insert deployment: {}", e);
            println!("Failed to insert deployment: {}", e);
            Err(anyhow::anyhow!("Failed to insert deployment: {}", e))
        }
    }
}

async fn read_db(query: Value) -> Result<Value, anyhow::Error> {
    let payload = ApiDeploymentsPayload {
        deployment_id: "".to_string(),
        query: query,
    };

    let payload = serde_json::json!({
        "event": "read_db",
        "table": "deployments",
        "data": payload
    });

    let response = match run_lambda(payload).await {
        Ok(response) => response,
        Err(e) => {
            error!("Failed to read db: {}", e);
            println!("Failed to read db: {}", e);
            return Err(anyhow::anyhow!("Failed to read db: {}", e));
        }
    };

    Ok(response)
}
