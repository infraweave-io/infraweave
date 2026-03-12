use std::collections::HashSet;

use env_defs::{get_deployment_identifier, CloudProvider, DeploymentResp};
use env_utils::merge_json_dicts;

use crate::interface::GenericCloudHandler;

fn get_payload(deployment: &DeploymentResp, is_plan: bool) -> serde_json::Value {
    let pk_prefix: &str = match is_plan {
        true => "PLAN",
        false => "DEPLOYMENT",
    };
    let pk = format!(
        "{}#{}",
        pk_prefix,
        get_deployment_identifier(
            &deployment.project_id,
            &deployment.region,
            &deployment.deployment_id,
            &deployment.environment
        )
    );

    let sk = match is_plan {
        true => &deployment.job_id,
        false => "METADATA",
    };

    let deleted = if deployment.deleted { 1 } else { 0 };
    let deleted_pk = format!("{}|{}", deleted, pk);
    let deleted_sk = format!(
        "{}|{}#{}",
        deleted,
        sk,
        get_deployment_identifier(&deployment.project_id, &deployment.region, "", "")
    );
    let deleted_pk_base = deleted_pk
        .split("::")
        .take(2)
        .collect::<Vec<&str>>()
        .join("::");
    let module_pk_base = format!(
        "MODULE#{}#{}",
        get_deployment_identifier(&deployment.project_id, &deployment.region, "", ""),
        deployment.module
    );

    // Prepare the DynamoDB payload for deployment metadata (including composite keys for indices)
    let mut deployment_payload = serde_json::to_value(serde_json::json!({
        "PK": pk,
        "SK": sk,
        "deleted_PK": deleted_pk,
        "deleted_PK_base": deleted_pk_base,
        "deleted_SK_base": deleted_sk,
        "module_PK_base": module_pk_base,
    }))
    .unwrap();
    let deployment_value = serde_json::to_value(deployment).unwrap();
    merge_json_dicts(&mut deployment_payload, &deployment_value);
    deployment_payload["deleted"] = serde_json::json!(if deployment.deleted { 1 } else { 0 }); // AWS specific: Boolean is not supported in GSI, so convert it to/from int for AWS
    deployment_payload
}

pub async fn set_deployment(
    handler: &GenericCloudHandler,
    deployment: &DeploymentResp,
    is_plan: bool,
) -> Result<(), anyhow::Error> {
    let deployment_table_placeholder = "deployments";

    // Prepare transaction items
    let mut transaction_items = vec![];

    // Fetch existing dependencies (needed in both cases)
    let existing_dependencies = match handler
        .get_deployment(&deployment.deployment_id, &deployment.environment, false)
        .await
    {
        Ok(deployment) => match deployment {
            Some(deployment) => deployment.dependencies,
            None => vec![],
        },
        Err(e) => {
            return Err(anyhow::anyhow!(
                "Failed to get deployment to find dependents: {}",
                e
            ))
        }
    };

    let deployment_payload = get_payload(deployment, is_plan);

    transaction_items.push(serde_json::json!({
        "Put": {
            "TableName": deployment_table_placeholder,
            "Item": deployment_payload
        }
    }));

    if !is_plan {
        if deployment.deleted {
            // -------------------------
            // Deletion Logic
            // -------------------------

            // Fetch all DEPENDENT items under the deployment's PK
            let dependent_sks = handler
                .get_dependents(&deployment.deployment_id, &deployment.environment)
                .await?;

            // Delete DEPENDENT items under the deployment's PK
            for dependent in dependent_sks {
                let pk = deployment_payload["PK"].as_str().unwrap().to_string();
                transaction_items.push(serde_json::json!({
                    "Delete": {
                        "TableName": deployment_table_placeholder,
                        "Key": {
                            "PK": pk.clone(),
                            "SK": format!("DEPENDENT#{}", get_deployment_identifier(&dependent.project_id, &dependent.region, &dependent.dependent_id, &dependent.environment)),
                        }
                    }
                }));
            }

            // Delete DEPENDENT items under dependencies' PKs
            for dependency in existing_dependencies.iter() {
                let dependency_pk = format!(
                    "DEPLOYMENT#{}",
                    get_deployment_identifier(
                        &dependency.project_id,
                        &dependency.region,
                        &dependency.deployment_id,
                        &dependency.environment
                    )
                );
                transaction_items.push(serde_json::json!({
                    "Delete": {
                        "TableName": deployment_table_placeholder,
                        "Key": {
                            "PK": dependency_pk,
                            "SK": format!("DEPENDENT#{}", get_deployment_identifier(&deployment.project_id, &deployment.region, &deployment.deployment_id, &deployment.environment)),
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
                        get_deployment_identifier(
                            &d.project_id,
                            &d.region,
                            &d.deployment_id,
                            &d.environment
                        )
                    )
                })
                .collect();

            let new_dependency_set: HashSet<String> = deployment
                .dependencies
                .iter()
                .map(|d| {
                    format!(
                        "DEPLOYMENT#{}",
                        get_deployment_identifier(
                            &d.project_id,
                            &d.region,
                            &d.deployment_id,
                            &d.environment
                        )
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
                        "TableName": deployment_table_placeholder, // TODO: Use Dependent class
                        "Item": {
                            "PK": dependency_pk.clone(),
                            "SK": format!("DEPENDENT#{}", get_deployment_identifier(&deployment.project_id, &deployment.region, &deployment.deployment_id, &deployment.environment)),
                            "dependent_id": deployment.deployment_id,
                            "module": deployment.module,
                            "environment": deployment.environment,
                            "project_id": deployment.project_id,
                            "region": deployment.region,
                        }
                    }
                }));
            }

            // Remove old DEPENDENT items
            for dependency_pk in dependencies_to_remove {
                transaction_items.push(serde_json::json!({
                    "Delete": {
                        "TableName": deployment_table_placeholder,
                        "Key": {
                            "PK": dependency_pk.clone(),
                            "SK": format!("DEPENDENT#{}", get_deployment_identifier(&deployment.project_id, &deployment.region, &deployment.deployment_id, &deployment.environment)),
                        }
                    }
                }));
            }
        }
    }

    // -------------------------
    // Execute the Transaction
    // -------------------------
    let items = serde_json::to_value(&transaction_items)?;
    let payload = env_defs::transact_write_event(&items);

    match handler.run_function(&payload).await {
        Ok(_) => Ok(()),
        Err(e) => Err(anyhow::anyhow!("Failed to update deployment: {}", e)),
    }?;

    if is_plan && deployment.has_drifted {
        let updated_deployment_payload = get_payload(deployment, false);
        match handler.run_function(&updated_deployment_payload).await {
            Ok(_) => Ok(()),
            Err(e) => Err(anyhow::anyhow!("Failed to update deployment: {}", e)),
        }?;
    }

    Ok(())
}
