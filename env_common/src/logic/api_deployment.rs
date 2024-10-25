use std::collections::HashSet;

use env_defs::{Dependent, DeploymentResp};
use env_utils::merge_json_dicts;

use crate::interface::CloudHandler;

use super::common::handler;

fn get_identifier(deployment_id: &str, environment: &str) -> String {
    format!("{}::{}", environment, deployment_id)
}

pub async fn get_all_deployments(environment: &str) -> Result<Vec<DeploymentResp>, anyhow::Error> {
    handler().get_all_deployments(environment).await
}

pub async fn get_deployment_and_dependents(deployment_id: &str, environment: &str, include_deleted: bool) -> Result<(Option<DeploymentResp>, Vec<Dependent>), anyhow::Error> {
    handler().get_deployment_and_dependents(deployment_id, environment, include_deleted).await
}

pub async fn get_deployment(deployment_id: &str, environment: &str, include_deleted: bool) -> Result<Option<DeploymentResp>, anyhow::Error> {
    handler().get_deployment(deployment_id, environment, include_deleted).await
}

pub async fn get_deployments_using_module(module: &str) -> Result<Vec<DeploymentResp>, anyhow::Error> {
    handler().get_deployments_using_module(module).await
}

pub async fn get_plan_deployment(deployment_id: &str, environment: &str, job_id: &str) -> Result<Option<DeploymentResp>, anyhow::Error> {
    handler().get_plan_deployment(deployment_id, environment, job_id).await
}

pub async fn set_deployment(deployment: DeploymentResp, is_plan: bool) -> Result<(), anyhow::Error> {
    let DEPLOYMENT_TABLE_NAME = "Deployments-eu-central-1-dev"; // TODO make placeholder to be replaced in lambda
    let pk_prefix: &str = match is_plan {
        true => "PLAN",
        false => "DEPLOYMENT",
    };
    let pk = format!(
        "{}#{}",
        pk_prefix,
        get_identifier(&deployment.deployment_id, &deployment.environment)
    );

    // Prepare transaction items
    let mut transaction_items = vec![];

    // Fetch existing dependencies (needed in both cases)
    let existing_dependencies = match handler().get_deployment(&deployment.deployment_id, &deployment.environment, false).await {
        Ok(deployment) => match deployment {
            Some(deployment) => deployment.dependencies,
            None => vec![],
        },
        Err(e) => return Err(anyhow::anyhow!("Failed to get deployment to find dependents: {}", e)),
    };

    let sk = match is_plan {
        true => &deployment.job_id,
        false => "METADATA",
    };

    let deleted_pk = format!("{}|{}", if deployment.deleted { 1 } else { 0 }, pk);

    // Prepare the DynamoDB payload for deployment metadata
    let mut deployment_payload = serde_json::to_value(serde_json::json!({
        "PK": pk,
        "SK": sk,
        "deleted_PK": deleted_pk,
    })).unwrap();
    let deployment_value = serde_json::to_value(&deployment).unwrap();
    merge_json_dicts(&mut deployment_payload, &deployment_value);
    deployment_payload["deleted"] = serde_json::json!(if deployment.deleted { 1 } else { 0 }); // AWS specific: Boolean is not supported in GSI, so convert it to/from int for AWS

    // Update deployment metadata
    transaction_items.push(serde_json::json!({
        "Put": {
            "TableName": DEPLOYMENT_TABLE_NAME,
            "Item": deployment_payload
        }
    }));

    if !is_plan {
        if deployment.deleted {
            // -------------------------
            // Deletion Logic
            // -------------------------

            // Fetch all DEPENDENT items under the deployment's PK
            let dependent_sks = handler().get_dependents(
                &deployment.deployment_id,
                &deployment.environment,
            )
            .await?;

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
    }

    // -------------------------
    // Execute the Transaction
    // -------------------------
    let payload = serde_json::json!({
        "event": "transact_write",
        "items": transaction_items,
    });

    println!("Invoking Lambda with payload: {}", payload);

    match handler().run_function(&payload).await {
        Ok(_) => Ok(()),
        Err(e) => {
            Err(anyhow::anyhow!("Failed to update deployment: {}", e))
        }
    }
}
