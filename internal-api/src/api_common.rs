// Common API route implementations that work for both AWS and Azure
use anyhow::{anyhow, Result};
use base64::{engine::general_purpose, Engine as _};
use futures::future::join_all;
use serde_json::Value;

pub trait DatabaseQuery {
    async fn query_table(
        &self,
        container: &str,
        query: &Value,
        region: Option<&str>,
    ) -> Result<Value>;
}

// Helper macro to extract required parameters from payload
#[macro_export]
macro_rules! get_param {
    ($payload:expr, $name:expr) => {
        $payload
            .get($name)
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!(concat!("Missing '", $name, "' parameter")))?
    };
}

// Helper to query and return all items
async fn query_all<Q: DatabaseQuery>(
    db: &Q,
    container: &str,
    mut query: Value,
    payload: Option<&Value>,
) -> Result<Value> {
    let mut region = None;

    if let Some(payload) = payload {
        if let Some(r) = payload.get("region").and_then(|v| v.as_str()) {
            region = Some(r);
        }

        if let Some(limit) = payload.get("limit").and_then(|v| v.as_i64()) {
            query["Limit"] = serde_json::json!(limit);
        }

        if let Some(next_token) = payload.get("next_token").and_then(|v| v.as_str()) {
            if let Ok(decoded) = general_purpose::STANDARD.decode(next_token) {
                if let Ok(json_str) = String::from_utf8(decoded) {
                    if let Ok(key) = serde_json::from_str::<Value>(&json_str) {
                        query["ExclusiveStartKey"] = key;
                    }
                }
            }
        }
    }
    db.query_table(container, &query, region).await
}

// Helper to query and return first item or error if not found
async fn query_one<Q: DatabaseQuery>(
    db: &Q,
    container: &str,
    query: Value,
    region: Option<&str>,
) -> Result<Value> {
    let response = db.query_table(container, &query, region).await?;
    let items = response
        .get("Items")
        .and_then(|v| v.as_array())
        .ok_or_else(|| anyhow!("Invalid response from query_table"))?;

    items
        .first()
        .map(|v| v.clone())
        .ok_or_else(|| anyhow!("Item not found"))
}

pub async fn describe_deployment_impl<Q: DatabaseQuery>(
    db: &Q,
    payload: &Value,
    qb: impl Fn(&str, &str, &str, &str, bool) -> Value,
) -> Result<Value> {
    query_one(
        db,
        "deployments",
        qb(
            get_param!(payload, "project"),
            get_param!(payload, "region"),
            get_param!(payload, "deployment_id"),
            get_param!(payload, "environment"),
            true,
        ),
        payload.get("region").and_then(|v| v.as_str()),
    )
    .await
    .map_err(|_| anyhow!("Deployment not found"))
}

pub async fn get_plan_deployment_impl<Q: DatabaseQuery>(
    db: &Q,
    payload: &Value,
    qb: impl Fn(&str, &str, &str, &str, &str) -> Value,
) -> Result<Value> {
    query_one(
        db,
        "deployments",
        qb(
            get_param!(payload, "project"),
            get_param!(payload, "region"),
            get_param!(payload, "deployment_id"),
            get_param!(payload, "environment"),
            get_param!(payload, "job_id"),
        ),
        payload.get("region").and_then(|v| v.as_str()),
    )
    .await
    .map_err(|_| anyhow!("Plan deployment not found"))
}

pub async fn get_deployments_impl<Q: DatabaseQuery>(
    db: &Q,
    payload: &Value,
    qb: impl Fn(&str, &str, &str, bool) -> Value,
) -> Result<Value> {
    let region = get_param!(payload, "region");
    let include_deleted = payload
        .get("include_deleted")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    if let Some(projects) = payload.get("projects").and_then(|v| v.as_array()) {
        let futures = projects.iter().filter_map(|p| p.as_str()).map(|project| {
            let query = qb(project, region, "", include_deleted);
            query_all(db, "deployments", query, Some(payload))
        });

        let results = join_all(futures).await;

        let mut all_items = Vec::new();
        for res in results {
            let val = res?;
            if let Some(items) = val.get("Items").and_then(|i| i.as_array()) {
                all_items.extend(items.clone());
            }
        }

        Ok(serde_json::json!({
            "Items": all_items,
            "Count": all_items.len()
        }))
    } else {
        query_all(
            db,
            "deployments",
            qb(get_param!(payload, "project"), region, "", include_deleted),
            Some(payload),
        )
        .await
    }
}

pub async fn get_modules_impl<Q: DatabaseQuery>(
    db: &Q,
    payload: &Value,
    qb: impl Fn(&str, bool, bool) -> Value,
) -> Result<Value> {
    let include_deprecated = payload
        .get("include_deprecated")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let include_dev000 = payload
        .get("include_dev000")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);
    query_all(
        db,
        "modules",
        qb("", include_deprecated, include_dev000),
        Some(payload),
    )
    .await
}

pub async fn get_projects_impl<Q: DatabaseQuery>(
    db: &Q,
    payload: &Value,
    qb: impl Fn() -> Value,
) -> Result<Value> {
    query_all(db, "config", qb(), Some(payload)).await
}

pub async fn get_stacks_impl<Q: DatabaseQuery>(
    db: &Q,
    payload: &Value,
    qb: impl Fn(&str, bool, bool) -> Value,
) -> Result<Value> {
    let include_deprecated = payload
        .get("include_deprecated")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let include_dev000 = payload
        .get("include_dev000")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);
    query_all(
        db,
        "modules",
        qb("", include_deprecated, include_dev000),
        Some(payload),
    )
    .await
}

pub async fn get_providers_impl<Q: DatabaseQuery>(
    db: &Q,
    payload: &Value,
    qb: impl Fn() -> Value,
) -> Result<Value> {
    query_all(db, "modules", qb(), Some(payload)).await
}

pub async fn get_policies_impl<Q: DatabaseQuery>(
    db: &Q,
    payload: &Value,
    qb: impl Fn(&str) -> Value,
) -> Result<Value> {
    query_all(
        db,
        "policies",
        qb(get_param!(payload, "environment")),
        Some(payload),
    )
    .await
}

pub async fn get_policy_version_impl<Q: DatabaseQuery>(
    db: &Q,
    payload: &Value,
    qb: impl Fn(&str, &str, &str) -> Value,
) -> Result<Value> {
    query_one(
        db,
        "policies",
        qb(
            get_param!(payload, "policy_name"),
            get_param!(payload, "environment"),
            get_param!(payload, "policy_version"),
        ),
        None,
    )
    .await
    .map_err(|_| anyhow!("Policy not found"))
}

pub async fn get_module_version_impl<Q: DatabaseQuery>(
    db: &Q,
    payload: &Value,
    qb: impl Fn(&str, &str, &str) -> Value,
) -> Result<Value> {
    query_one(
        db,
        "modules",
        qb(
            get_param!(payload, "module_name"),
            get_param!(payload, "track"),
            get_param!(payload, "module_version"),
        ),
        None,
    )
    .await
    .map_err(|_| anyhow!("Module not found"))
}

pub async fn get_provider_version_impl<Q: DatabaseQuery>(
    db: &Q,
    payload: &Value,
    qb: impl Fn(&str, &str) -> Value,
) -> Result<Value> {
    query_one(
        db,
        "modules",
        qb(
            get_param!(payload, "provider"),
            get_param!(payload, "version"),
        ),
        None,
    )
    .await
    .map_err(|_| anyhow!("Provider not found"))
}

pub async fn get_stack_version_impl<Q: DatabaseQuery>(
    db: &Q,
    payload: &Value,
    qb: impl Fn(&str, &str, &str) -> Value,
) -> Result<Value> {
    query_one(
        db,
        "modules",
        qb(
            get_param!(payload, "stack_name"),
            get_param!(payload, "track"),
            get_param!(payload, "stack_version"),
        ),
        None,
    )
    .await
    .map_err(|_| anyhow!("Stack not found"))
}

pub async fn get_all_versions_for_module_impl<Q: DatabaseQuery>(
    db: &Q,
    payload: &Value,
    qb: impl Fn(&str, &str, bool, bool) -> Value,
) -> Result<Value> {
    let include_deprecated = payload
        .get("include_deprecated")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let include_dev000 = payload
        .get("include_dev000")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);
    query_all(
        db,
        "modules",
        qb(
            get_param!(payload, "module"),
            get_param!(payload, "track"),
            include_deprecated,
            include_dev000,
        ),
        Some(payload),
    )
    .await
}

pub async fn get_all_versions_for_stack_impl<Q: DatabaseQuery>(
    db: &Q,
    payload: &Value,
    qb: impl Fn(&str, &str, bool, bool) -> Value,
) -> Result<Value> {
    let include_deprecated = payload
        .get("include_deprecated")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let include_dev000 = payload
        .get("include_dev000")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);
    query_all(
        db,
        "modules",
        qb(
            get_param!(payload, "stack"),
            get_param!(payload, "track"),
            include_deprecated,
            include_dev000,
        ),
        Some(payload),
    )
    .await
}

pub async fn get_deployments_for_module_impl<Q: DatabaseQuery>(
    db: &Q,
    payload: &Value,
    qb: impl Fn(&str, &str, &str, &str, bool) -> Value,
) -> Result<Value> {
    let include_deleted = payload
        .get("include_deleted")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    if let Some(projects) = payload.get("projects").and_then(|v| v.as_array()) {
        let region = get_param!(payload, "region");
        let module = get_param!(payload, "module");

        let futures: Vec<_> = projects
            .iter()
            .filter_map(|p| p.as_str())
            .map(|project| {
                let query_val = qb(project, region, module, "", include_deleted);
                query_all(db, "deployments", query_val, Some(payload))
            })
            .collect();

        let results = futures::future::join_all(futures).await;

        // Aggregate results
        let mut all_items = Vec::new();
        for res in results {
            let val = res?;
            if let Some(items) = val.get("Items").and_then(|i| i.as_array()) {
                all_items.extend(items.clone());
            }
        }

        Ok(serde_json::json!({
            "Items": all_items,
            "Count": all_items.len()
        }))
    } else {
        query_all(
            db,
            "deployments",
            qb(
                get_param!(payload, "project"),
                get_param!(payload, "region"),
                get_param!(payload, "module"),
                "",
                include_deleted,
            ),
            Some(payload),
        )
        .await
    }
}

pub async fn get_events_impl<Q: DatabaseQuery>(
    db: &Q,
    payload: &Value,
    qb: impl Fn(&str, &str, &str, &str, Option<&str>) -> Value,
) -> Result<Value> {
    query_all(
        db,
        "events",
        qb(
            get_param!(payload, "project"),
            get_param!(payload, "region"),
            get_param!(payload, "deployment_id"),
            get_param!(payload, "environment"),
            payload.get("event_type").and_then(|v| v.as_str()),
        ),
        Some(payload),
    )
    .await
}

pub async fn get_change_record_impl<Q: DatabaseQuery>(
    db: &Q,
    payload: &Value,
    qb: impl Fn(&str, &str, &str, &str, &str, &str) -> Value,
) -> Result<Value> {
    let project = get_param!(payload, "project");
    let region = get_param!(payload, "region");
    let environment = get_param!(payload, "environment");
    let deployment_id = get_param!(payload, "deployment_id");
    let job_id = get_param!(payload, "job_id");
    let change_type = get_param!(payload, "change_type");

    // Normalize change_type to PK prefix (same logic as insertion)
    // This handles both lowercase ("plan") and uppercase ("PLAN") inputs
    let pk_prefix = match change_type.to_lowercase().as_str() {
        "apply" | "destroy" | "mutate" => "MUTATE",
        "plan" => "PLAN",
        _ => change_type, // fallback to original if unknown
    };

    log::info!("get_change_record_impl: project={}, region={}, env={}, dep_id={}, job_id={}, change_type={} -> pk_prefix={}", 
        project, region, environment, deployment_id, job_id, change_type, pk_prefix);

    let mut result = if pk_prefix != "PLAN" {
        let query = qb(
            project,
            region,
            environment,
            deployment_id,
            job_id,
            "MUTATE",
        );
        log::info!(
            "Querying with MUTATE, query: {}",
            serde_json::to_string_pretty(&query).unwrap_or_default()
        );
        query_one(db, "change_records", query, Some(region)).await
    } else {
        Err(anyhow!("Skipping MUTATE for PLAN"))
    };

    if result.is_err() {
        let query = qb(
            project,
            region,
            environment,
            deployment_id,
            job_id,
            pk_prefix,
        );
        log::info!(
            "Querying with pk_prefix={}, query: {}",
            pk_prefix,
            serde_json::to_string_pretty(&query).unwrap_or_default()
        );
        result = query_one(db, "change_records", query, Some(region)).await;
    }

    result.map_err(|e| {
        log::error!("Change record not found: {}", e);
        anyhow!("Change record not found")
    })
}
pub async fn get_deployment_history_impl<Q: DatabaseQuery>(
    db: &Q,
    payload: &Value,
    plans_qb: impl Fn(&str, &str, Option<&str>) -> Value,
    deleted_qb: impl Fn(&str, &str, Option<&str>) -> Value,
) -> Result<Value> {
    let project = get_param!(payload, "project");
    let region = get_param!(payload, "region");
    let environment = payload.get("environment").and_then(|v| v.as_str());
    let history_type = get_param!(payload, "type");

    log::info!(
        "get_deployment_history_impl: project={}, region={}, env={:?}, type={}",
        project,
        region,
        environment,
        history_type
    );

    let query = match history_type {
        "plans" => plans_qb(project, region, environment),
        "deleted" => deleted_qb(project, region, environment),
        _ => {
            return Err(anyhow!(
                "Invalid type parameter. Must be 'plans' or 'deleted'"
            ))
        }
    };

    log::info!(
        "Query: {}",
        serde_json::to_string_pretty(&query).unwrap_or_default()
    );

    let mut result = query_all(db, "deployments", query, Some(payload)).await?;

    // Sort results by epoch in descending order
    if let Some(items) = result.get_mut("Items").and_then(|v| v.as_array_mut()) {
        items.sort_by(|a, b| {
            let epoch_a = a.get("epoch").and_then(|v| v.as_u64()).unwrap_or(0);
            let epoch_b = b.get("epoch").and_then(|v| v.as_u64()).unwrap_or(0);
            epoch_b.cmp(&epoch_a) // Descending order (newest first)
        });
    }

    Ok(result)
}
