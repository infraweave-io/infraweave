use std::collections::HashSet;

use env_defs::{Dependency, Dependent, DeploymentResp};
use env_utils::merge_json_dicts;
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

pub async fn describe_deployment_id(deployment_id: &str, environment: &str) -> anyhow::Result<(DeploymentResp, Vec<Dependent>)> {
    let pk = format!(
        "DEPLOYMENT#{}",
        get_identifier(deployment_id,  environment)
    );

    let query = serde_json::json!({
        "TableName": "Deployments-eu-central-1-dev",
        "KeyConditionExpression": "PK = :pk",
        "FilterExpression": "deleted <> :deleted",
        "ExpressionAttributeValues": {
            ":pk": pk,
            ":deleted": 1
        }
    });
    let response = read_db(query).await?;
    let items = response.get("Items").ok_or_else(|| anyhow::anyhow!("No Items field in response"))?;

    if let Some(elements) = items.as_array() {
        let mut deployments_vec: Vec<DeploymentResp> = vec![];
        let mut dependents_vec: Vec<Dependent> = vec![];
        for e in elements {
            if e.get("SK").unwrap().as_str().unwrap().starts_with("DEPENDENT#") {
                let dependent: Dependent = serde_json::from_value(e.clone()).expect("Failed to parse dependent");
                dependents_vec.push(dependent);
            } else {
                deployments_vec.push(map_to_deployment(e.clone()));
            }
        }
        if deployments_vec.len() != 1 {
            println!("Found {} deployments", deployments_vec.len());
            println!("Found {} dependents", dependents_vec.len());
            panic!("Expected exactly one deployment");
        }
        println!("Describing deployment id:");
        print_api_resources(vec![deployments_vec[0].clone()]);
        return Ok((deployments_vec[0].clone(), dependents_vec));
    } else {
        panic!("Expected an array of deployments");
    }
}

async fn get_all_deployments() -> anyhow::Result<Vec<DeploymentResp>> {
    let response = read_db(serde_json::json!({
        "IndexName": "DeletedIndex",
        "KeyConditionExpression": "deleted = :deleted",
        "ExpressionAttributeValues": {
            ":deleted": 0
        }
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
    let mut value = value.clone();
    value["deleted"] = serde_json::json!(value["deleted"].as_f64().unwrap() != 0.0); // Boolean is not supported in GSI, so convert it to/from int for AWS
    // Idea for backwards compatibility:
    // e.g. if missing keys policy_results and output, add them
    if value.get("policy_results").is_none() {
        value["policy_results"] = serde_json::json!(Value::Null);
    }
    serde_json::from_value(value).unwrap()
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

pub async fn set_deployment(deployment: DeploymentResp) -> anyhow::Result<String> {
    let DEPLOYMENT_TABLE_NAME = "Deployments-eu-central-1-dev";
    let pk = format!(
        "DEPLOYMENT#{}",
        get_identifier(&deployment.deployment_id, &deployment.environment)
    );

    // Prepare transaction items
    let mut transaction_items = vec![];
    let action;

    // Fetch existing dependencies (needed in both cases)
    let existing_dependencies = get_existing_dependencies(
        &deployment.deployment_id,
        &deployment.environment,
    )
    .await?;

    // Prepare the DynamoDB payload for deployment metadata
    let mut deployment_payload = serde_json::to_value(serde_json::json!({
        "PK": pk.clone(),
        "SK": "METADATA",
    })).unwrap();
    let deployment_value = serde_json::to_value(&deployment).unwrap();
    merge_json_dicts(&mut deployment_payload, &deployment_value);
    deployment_payload["deleted"] = serde_json::json!(if deployment.deleted { 1 } else { 0 }); // AWS specific: Boolean is not supported in GSI, so convert it to/from int for AWS

    if deployment.deleted {
        // -------------------------
        // Deletion Logic
        // -------------------------
        action = "delete";

        // Fetch all DEPENDENT items under the deployment's PK
        let dependent_sks = get_dependents(
            &deployment.deployment_id,
            &deployment.environment,
        )
        .await?;
    
        // Update deployment metadata
        transaction_items.push(serde_json::json!({
            "Put": {
                "TableName": DEPLOYMENT_TABLE_NAME,
                "Item": deployment_payload
            }
        }));

        // Delete DEPENDENT items under the deployment's PK
        for dependent in dependent_sks {
            transaction_items.push(serde_json::json!({
                "Delete": {
                    "TableName": DEPLOYMENT_TABLE_NAME,
                    "Key": {
                        "PK": pk.clone(),
                        "SK": format!("DEPENDENT#{}", get_identifier(&dependent.dependent_id, &dependent.environment)),
                    }
                }
            }));
        }

        // Delete DEPENDENT items under dependencies' PKs
        for dependency in existing_dependencies.iter() {
            let dependency_pk = format!(
                "DEPLOYMENT#{}",
                get_identifier(&dependency.deployment_id, &dependency.environment)
            );
            transaction_items.push(serde_json::json!({
                "Delete": {
                    "TableName": DEPLOYMENT_TABLE_NAME,
                    "Key": {
                        "PK": dependency_pk,
                        "SK": format!("DEPENDENT#{}", get_identifier(&deployment.deployment_id, &deployment.environment)),
                    }
                }
            }));
        }
    } else {
        // -------------------------
        // Insertion/Update Logic
        // -------------------------
        action = "insert";

        // Convert dependencies into sets for comparison
        let old_dependency_set: HashSet<String> = existing_dependencies
            .iter()
            .map(|d| {
                format!(
                    "DEPLOYMENT#{}",
                    get_identifier(&d.deployment_id, &d.environment)
                )
            })
            .collect();

        let new_dependency_set: HashSet<String> = deployment
            .dependencies
            .iter()
            .map(|d| {
                format!(
                    "DEPLOYMENT#{}",
                    get_identifier(&d.deployment_id, &d.environment)
                )
            })
            .collect();

        // Identify dependencies to be added and removed
        let dependencies_to_add = new_dependency_set.difference(&old_dependency_set);
        let dependencies_to_remove = old_dependency_set.difference(&new_dependency_set);

        // Update deployment metadata
        transaction_items.push(serde_json::json!({
            "Put": {
                "TableName": DEPLOYMENT_TABLE_NAME,
                "Item": deployment_payload
            }
        }));

        // Add new DEPENDENT items
        for dependency_pk in dependencies_to_add {
            transaction_items.push(serde_json::json!({
                "Put": {
                    "TableName": DEPLOYMENT_TABLE_NAME,
                    "Item": {
                        "PK": dependency_pk.clone(),
                        "SK": format!("DEPENDENT#{}", get_identifier(&deployment.deployment_id, &deployment.environment)),
                        "dependent_id": deployment.deployment_id,
                        "module": deployment.module,
                        "environment": deployment.environment,
                    }
                }
            }));
        }

        // Remove old DEPENDENT items
        for dependency_pk in dependencies_to_remove {
            transaction_items.push(serde_json::json!({
                "Delete": {
                    "TableName": DEPLOYMENT_TABLE_NAME,
                    "Key": {
                        "PK": dependency_pk.clone(),
                        "SK": format!("DEPENDENT#{}", get_identifier(&deployment.deployment_id, &deployment.environment)),
                    }
                }
            }));
        }
    }

    // -------------------------
    // Execute the Transaction
    // -------------------------
    let payload = serde_json::json!({
        "event": "transact_write",
        "items": transaction_items,
    });

    println!("Invoking Lambda with payload: {}", payload);

    match run_lambda(payload).await {
        Ok(_) => Ok("".to_string()),
        Err(e) => {
            error!("Failed to {} deployment: {}", action, e);
            Err(anyhow::anyhow!("Failed to {} deployment: {}", action, e))
        }
    }
}

async fn get_dependents(deployment_id: &str, environment: &str) -> anyhow::Result<Vec<Dependent>> {
    let pk = format!(
        "DEPLOYMENT#{}",
        get_identifier(deployment_id,  environment)
    );

    let query = serde_json::json!({
        "TableName": "Deployments-eu-central-1-dev",
        "KeyConditionExpression": "PK = :pk AND begins_with(SK, :dependent_prefix)",
        "FilterExpression": "deleted = :deleted",
        "ExpressionAttributeValues": {
            ":pk": pk,
            ":dependent_prefix": "DEPENDENT#",
            ":deleted": 0
        }
    });

    let response = read_db(query).await?;
    let items = response.get("Items").ok_or_else(|| anyhow::anyhow!("No Items field in response"))?;

    let dependent_sks = items.as_array().unwrap_or(&Vec::new())
        .iter()
        .map(|item| {
            let dependent: Dependent = serde_json::from_value(item.clone()).expect("Failed to parse dependent");
            Ok(dependent)
        })
        .collect::<Result<Vec<Dependent>, anyhow::Error>>().unwrap();

    Ok(dependent_sks)
}


async fn get_existing_dependencies(
    deployment_id: &str,
    environment: &str,
) -> anyhow::Result<Vec<Dependency>> {
    let pk = format!(
        "DEPLOYMENT#{}",
        get_identifier(deployment_id, environment)
    );

    let query: Value = serde_json::json!({
        "TableName": "Deployments-eu-central-1-dev",
        "KeyConditionExpression": "PK = :pk AND SK = :metadata",
        "FilterExpression": "deleted = :deleted",
        "ExpressionAttributeValues": {
            ":pk": pk,
            ":metadata": "METADATA",
            ":deleted": 0
        }
    });

    // Use `read_db` to execute the query
    let response = match read_db(query).await {
        Ok(res) => res,
        Err(e) => {
            error!("Failed to fetch existing dependencies: {}", e);
            return Err(anyhow::anyhow!(
                "Failed to fetch existing dependencies: {}",
                e
            ));
        }
    };

    let items = response
        .get("Items")
        .ok_or_else(|| anyhow::anyhow!("No Items field in DynamoDB response"))?;

    if items.as_array().unwrap_or(&Vec::new()).is_empty() {
        return Ok(Vec::new());
    }

    // Parse the first (and only) deployment metadata item
    let item = &items.as_array().unwrap()[0];

    let dependencies = match item.get("dependencies") {
        Some(dependencies_value) => {
            serde_json::from_value::<Vec<Dependency>>(dependencies_value.clone())
                .map_err(|e| anyhow::anyhow!("Failed to parse dependencies: {}", e))?
        }
        None => Vec::new(),
    };

    Ok(dependencies)
}

fn get_identifier(deployment_id: &str, environment: &str) -> String {
    format!("{}::{}", environment, deployment_id)
}
