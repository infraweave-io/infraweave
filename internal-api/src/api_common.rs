// Common API route implementations that work for both AWS and Azure
use anyhow::{anyhow, Result};
use base64::{engine::general_purpose, Engine as _};
use futures::stream::{self, StreamExt};
use serde_json::Value;

pub trait DatabaseQuery {
    async fn query_table(
        &self,
        container: &str,
        query: &Value,
        region: Option<&str>,
    ) -> Result<Value>;

    async fn query_workload_account(
        &self,
        workload_account: &str,
        container: &str,
        query: &Value,
        region: Option<&str>,
    ) -> Result<Value> {
        let _ = workload_account;
        self.query_table(container, query, region).await
    }
}

pub trait JobRunner {
    async fn liveness_check(&self, record: Value) -> Value;
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

async fn query_all_for_workload<Q: DatabaseQuery>(
    db: &Q,
    workload_account: &str,
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
    db.query_workload_account(workload_account, container, &query, region)
        .await
}

/// How many workload accounts a cross-account fan-out queries at once.
///
/// Each account costs an assume-role plus a query, and every account in one request
/// shares that request's timeout. Unbounded, an organisation with a few hundred
/// accounts overruns it and the caller sees a gateway error instead of a result.
const MAX_CONCURRENT_WORKLOAD_QUERIES: usize = 10;

/// Runs the same query in every listed account and concatenates the items.
///
/// An account that fails is logged and skipped instead of failing the whole response:
/// with hundreds of accounts in one request, one unreachable account would otherwise
/// hide every account that did answer. The number skipped rides back on
/// `partial_failures` so a caller can tell a short list from a complete one.
///
/// Per-account pagination tokens are dropped, since concatenated pages have no single
/// cursor that could resume the whole set.
async fn query_all_workload_accounts<Q, F>(
    db: &Q,
    projects: &[&str],
    container: &str,
    payload: &Value,
    build_query: F,
) -> Result<Value>
where
    Q: DatabaseQuery,
    F: Fn(&str) -> Value,
{
    let mut tasks = Vec::with_capacity(projects.len());
    for project in projects {
        let query = build_query(project);
        tasks.push(async move {
            let result = query_all_for_workload(db, project, container, query, Some(payload)).await;
            (*project, result)
        });
    }

    let mut responses = stream::iter(tasks).buffer_unordered(MAX_CONCURRENT_WORKLOAD_QUERIES);

    let mut all_items = Vec::new();
    let mut failures = 0usize;

    while let Some((project, result)) = responses.next().await {
        match result {
            Ok(val) => {
                if let Some(items) = val.get("Items").and_then(|i| i.as_array()) {
                    all_items.extend(items.iter().cloned());
                }
            }
            Err(e) => {
                failures += 1;
                log::warn!("Skipping project {project} while querying '{container}': {e}");
            }
        }
    }

    // Every account failing is not an empty result set; returning one would read as
    // "nothing is deployed" rather than "nothing could be read".
    if failures > 0 && all_items.is_empty() {
        return Err(anyhow!(
            "Query of '{}' failed for all {} projects",
            container,
            failures
        ));
    }

    let count = all_items.len();
    let mut response = serde_json::json!({
        "Items": all_items,
        "Count": count,
    });
    if failures > 0 {
        response["partial_failures"] = serde_json::json!(failures);
    }
    Ok(response)
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

async fn query_one_for_workload<Q: DatabaseQuery>(
    db: &Q,
    workload_account: &str,
    container: &str,
    query: Value,
    region: Option<&str>,
) -> Result<Value> {
    let response = db
        .query_workload_account(workload_account, container, &query, region)
        .await?;
    let items = response
        .get("Items")
        .and_then(|v| v.as_array())
        .ok_or_else(|| anyhow!("Invalid response from query_table"))?;

    items
        .first()
        .map(|v| v.clone())
        .ok_or_else(|| anyhow!("Item not found"))
}

pub async fn describe_deployment_impl<Q: DatabaseQuery + JobRunner>(
    db: &Q,
    payload: &Value,
    qb: impl Fn(&str, &str, &str, &str, bool) -> Value,
) -> Result<Value> {
    let project = get_param!(payload, "project");
    let region = get_param!(payload, "region");
    let record = query_one_for_workload(
        db,
        project,
        "deployments",
        qb(
            project,
            region,
            get_param!(payload, "deployment_id"),
            get_param!(payload, "environment"),
            true,
        ),
        Some(region),
    )
    .await
    .map_err(|_| anyhow!("Deployment not found"))?;
    Ok(db.liveness_check(record).await)
}

pub async fn get_plan_deployment_impl<Q: DatabaseQuery + JobRunner>(
    db: &Q,
    payload: &Value,
    qb: impl Fn(&str, &str, &str, &str, &str) -> Value,
) -> Result<Value> {
    let project = get_param!(payload, "project");
    let region = get_param!(payload, "region");
    let record = query_one_for_workload(
        db,
        project,
        "deployments",
        qb(
            project,
            region,
            get_param!(payload, "deployment_id"),
            get_param!(payload, "environment"),
            get_param!(payload, "job_id"),
        ),
        Some(region),
    )
    .await
    .map_err(|_| anyhow!("Plan deployment not found"))?;
    Ok(db.liveness_check(record).await)
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
        let project_ids: Vec<&str> = projects.iter().filter_map(|p| p.as_str()).collect();

        query_all_workload_accounts(db, &project_ids, "deployments", payload, |project| {
            qb(project, region, "", include_deleted)
        })
        .await
    } else {
        query_all_for_workload(
            db,
            get_param!(payload, "project"),
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

        let project_ids: Vec<&str> = projects.iter().filter_map(|p| p.as_str()).collect();

        query_all_workload_accounts(db, &project_ids, "deployments", payload, |project| {
            qb(project, region, module, "", include_deleted)
        })
        .await
    } else {
        query_all_for_workload(
            db,
            get_param!(payload, "project"),
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

/// Deployments of one module in every project the caller is allowed to see.
///
/// A single query on a module-partitioned index replaces the per-account fan-out of
/// [`get_deployments_for_module_impl`], which is what makes this usable for an
/// organisation with hundreds of accounts. The index cannot express who may see what,
/// so unlike the fan-out — where each account's credentials enforce it — the caller's
/// grants are applied to the result here.
///
/// The three cases follow [`crate::handlers::get_projects`], since a caller that can list
/// a project expects to be able to list its deployments: `allowed_projects` filters to
/// those projects; a `user_id` without it is a user whose token grants nothing, so the
/// answer is empty rather than everything; and no `user_id` at all means no authentication
/// is in play, which only happens in local builds because the route answers 401 without
/// one. Diverging here is how one endpoint ends up leaking what the other hides.
///
/// The query asks only for the attributes a list needs, so the caller sees a deployment
/// summary rather than a full deployment.
///
/// Because filtering happens after the page is read, a page can come back empty while
/// `next_token` still points at more: a caller must follow the token rather than stop
/// at the first empty page.
pub async fn get_deployments_for_module_globally_impl<Q: DatabaseQuery>(
    db: &Q,
    payload: &Value,
    qb: impl Fn(&str, &str, bool) -> Value,
) -> Result<Value> {
    let module = get_param!(payload, "module");
    let region = get_param!(payload, "region");
    query_token_scoped(db, payload, qb(module, region, include_deleted(payload))).await
}

/// Every deployment in a region, in every project the caller is allowed to see.
///
/// The fleet-wide counterpart of [`get_deployments_for_module_globally_impl`]: the
/// overview views need every module at once, and asking per module costs a request for
/// each one. Scoped by token in the same way, and items are the same summaries.
pub async fn get_fleet_deployments_impl<Q: DatabaseQuery>(
    db: &Q,
    payload: &Value,
    qb: impl Fn(&str, bool) -> Value,
) -> Result<Value> {
    let region = get_param!(payload, "region");
    query_token_scoped(db, payload, qb(region, include_deleted(payload))).await
}

fn include_deleted(payload: &Value) -> bool {
    payload
        .get("include_deleted")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
}

/// Database pages one token-scoped request reads before handing back a `next_token`.
///
/// A DynamoDB page is 1MB of *whole* items and the projection only trims them after
/// that read, so a page of summaries is a small response. Ten keeps one request well
/// inside the Lambda timeout.
const MAX_PAGES_PER_REQUEST: usize = 10;

/// Stop following pages once this many rows are collected, so a response of summaries
/// stays far below the Lambda and API Gateway payload limits.
const MAX_ITEMS_PER_REQUEST: usize = 3000;

/// Runs a query whose index spans every project, keeping only the rows of projects the
/// caller's token grants, and following several pages per request.
///
/// The three cases follow [`crate::handlers::get_projects`], since a caller that can list
/// a project expects to be able to list its deployments: `allowed_projects` filters to
/// those projects; a `user_id` without it is a user whose token grants nothing, so the
/// result is empty; neither means an unrestricted (local or internal) caller.
///
/// Each page is filtered after it is read, so a response can be empty and still carry a
/// `next_token`: callers must follow the token until it is absent. A caller that passes
/// `limit` gets a single page of at most that many rows, as before.
async fn query_token_scoped<Q: DatabaseQuery>(
    db: &Q,
    payload: &Value,
    query: Value,
) -> Result<Value> {
    let allowed_projects = match payload.get("allowed_projects").and_then(|v| v.as_array()) {
        Some(allowed) => Some(
            allowed
                .iter()
                .filter_map(|p| p.as_str())
                .collect::<Vec<&str>>(),
        ),
        None if payload.get("user_id").is_some() => {
            log::warn!(
                "User authenticated but no allowed_projects claim found - denying access to deployments"
            );
            Some(Vec::new())
        }
        None => None,
    };

    // Nothing is visible, so there is nothing worth reading.
    if allowed_projects.as_ref().is_some_and(|a| a.is_empty()) {
        return Ok(serde_json::json!({ "Items": [], "Count": 0 }));
    }

    let region = payload.get("region").and_then(|v| v.as_str());
    let limit = payload.get("limit").and_then(|v| v.as_i64());
    let max_pages = if limit.is_some() {
        1
    } else {
        MAX_PAGES_PER_REQUEST
    };
    let mut page = query_all(db, "deployments", query.clone(), Some(payload)).await?;
    let mut items: Vec<Value> = Vec::new();
    let mut pages = 1;

    loop {
        if let Some(page_items) = page.get_mut("Items").and_then(|i| i.as_array_mut()) {
            items.extend(page_items.drain(..).filter(|item| {
                allowed_projects.as_ref().is_none_or(|allowed| {
                    item.get("project_id")
                        .and_then(|v| v.as_str())
                        .is_some_and(|project| allowed.contains(&project))
                })
            }));
        }

        let Some(last_key) = page.get("LastEvaluatedKey").cloned() else {
            break;
        };
        if pages >= max_pages || items.len() >= MAX_ITEMS_PER_REQUEST {
            break;
        }

        let mut next = query.clone();
        next["ExclusiveStartKey"] = last_key;
        page = db.query_table("deployments", &next, region).await?;
        pages += 1;
    }

    // The last page read decides where the next request resumes; Count describes what
    // is being returned, so it follows the filtering.
    let count = items.len();
    let mut response = serde_json::json!({ "Items": items, "Count": count });
    if let Some(token) = page.get("next_token") {
        response["next_token"] = token.clone();
    }
    Ok(response)
}

pub async fn get_events_impl<Q: DatabaseQuery>(
    db: &Q,
    payload: &Value,
    qb: impl Fn(&str, &str, &str, &str, Option<&str>) -> Value,
) -> Result<Value> {
    query_all_for_workload(
        db,
        get_param!(payload, "project"),
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

pub async fn get_change_records_impl<Q: DatabaseQuery>(
    db: &Q,
    payload: &Value,
    qb: impl Fn(&str, &str, &str, &str, &str) -> Value,
) -> Result<Value> {
    let project = get_param!(payload, "project");
    let region = get_param!(payload, "region");
    let environment = get_param!(payload, "environment");
    let deployment_id = get_param!(payload, "deployment_id");
    let change_type = normalize_change_type(get_param!(payload, "change_type"));

    let prefixes = mutate_pk_prefixes(change_type);

    // Single prefix (e.g. PLAN): return verbatim, preserving pagination fields.
    if prefixes.len() == 1 {
        let query = qb(project, region, environment, deployment_id, prefixes[0]);
        return query_all_for_workload(db, project, "change_records", query, Some(payload)).await;
    }

    // MUTATE also reads legacy APPLY/DESTROY records; merge and re-sort by epoch.
    let mut items: Vec<Value> = Vec::new();
    for prefix in prefixes {
        let query = qb(project, region, environment, deployment_id, prefix);
        let resp =
            query_all_for_workload(db, project, "change_records", query, Some(payload)).await?;
        if let Some(arr) = resp.get("Items").and_then(|v| v.as_array()) {
            items.extend(arr.iter().cloned());
        }
    }
    items.sort_by_key(|i| std::cmp::Reverse(i.get("epoch").and_then(|v| v.as_u64()).unwrap_or(0)));
    if let Some(limit) = payload.get("limit").and_then(|v| v.as_i64()) {
        items.truncate(limit.max(0) as usize);
    }

    Ok(serde_json::json!({ "Count": items.len(), "Items": items }))
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

    // Normalize change_type to PK prefix; MUTATE also tries legacy APPLY/DESTROY.
    let prefixes = mutate_pk_prefixes(normalize_change_type(change_type));

    log::info!("get_change_record_impl: project={}, region={}, env={}, dep_id={}, job_id={}, change_type={} -> prefixes={:?}",
        project, region, environment, deployment_id, job_id, change_type, prefixes);

    let mut result = Err(anyhow!("Change record not found"));
    for prefix in prefixes {
        let query = qb(project, region, environment, deployment_id, job_id, prefix);
        result = query_one_for_workload(db, project, "change_records", query, Some(region)).await;
        if result.is_ok() {
            break;
        }
    }

    result.map_err(|e| {
        log::error!("Change record not found: {}", e);
        anyhow!("Change record not found")
    })
}

fn normalize_change_type(change_type: &str) -> &str {
    match change_type.to_lowercase().as_str() {
        "apply" | "destroy" | "mutate" => "MUTATE",
        "plan" => "PLAN",
        _ => change_type,
    }
}

/// PK prefixes to query for a change type. MUTATE unified the legacy
/// APPLY/DESTROY records, so it also reads those for backward compatibility.
/// TODO: drop the APPLY/DESTROY fallbacks once pre-MUTATE records have aged out.
fn mutate_pk_prefixes(pk_prefix: &str) -> Vec<&str> {
    match pk_prefix {
        "MUTATE" => vec!["MUTATE", "APPLY", "DESTROY"],
        other => vec![other],
    }
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

    let mut query = match history_type {
        "plans" => plans_qb(project, region, environment),
        "deleted" => deleted_qb(project, region, environment),
        _ => {
            return Err(anyhow!(
                "Invalid type parameter. Must be 'plans' or 'deleted'"
            ))
        }
    };
    let deployment_ids = history_deployment_ids(payload)?;
    narrow_to_deployment_ids(&mut query, &deployment_ids);

    log::info!(
        "Query: {}",
        serde_json::to_string_pretty(&query).unwrap_or_default()
    );

    let mut result = if deployment_ids.is_empty() {
        query_all_for_workload(db, project, "deployments", query, Some(payload)).await?
    } else {
        narrowed_history(db, project, query, payload).await?
    };

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

/// Safety net on the pages a narrowed history lookup follows.
const MAX_NARROWED_HISTORY_PAGES: usize = 500;

/// Every page of a history query narrowed to some deployments, followed here rather than by the
/// caller: the index is read to the end either way, but its pages then carry only the few
/// matches, so one response holds them all instead of a request per near-empty page.
async fn narrowed_history<Q: DatabaseQuery>(
    db: &Q,
    project: &str,
    query: Value,
    payload: &Value,
) -> Result<Value> {
    let mut page_payload = payload.clone();
    if let Some(obj) = page_payload.as_object_mut() {
        obj.remove("next_token");
        obj.remove("limit");
    }
    let mut items: Vec<Value> = Vec::new();
    for _ in 0..MAX_NARROWED_HISTORY_PAGES {
        let page = query_all_for_workload(
            db,
            project,
            "deployments",
            query.clone(),
            Some(&page_payload),
        )
        .await?;
        if let Some(page_items) = page.get("Items").and_then(|v| v.as_array()) {
            items.extend(page_items.iter().cloned());
        }
        match page.get("next_token").and_then(|v| v.as_str()) {
            Some(token) if !token.is_empty() => page_payload["next_token"] = Value::from(token),
            _ => return Ok(serde_json::json!({ "Items": items })),
        }
    }
    Err(anyhow!(
        "The history of {} has more than {} pages; narrow it to fewer deployments",
        project,
        MAX_NARROWED_HISTORY_PAGES
    ))
}

/// DynamoDB allows at most 100 values in an `IN` comparison.
const MAX_HISTORY_DEPLOYMENT_IDS: usize = 100;

/// The comma-separated `deployment_ids` a history request is narrowed to, if any.
fn history_deployment_ids(payload: &Value) -> Result<Vec<String>> {
    let mut ids: Vec<String> = Vec::new();
    for id in payload
        .get("deployment_ids")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .split(',')
        .map(str::trim)
        .filter(|id| !id.is_empty())
    {
        if !ids.iter().any(|known| known == id) {
            ids.push(id.to_string());
        }
    }
    if ids.len() > MAX_HISTORY_DEPLOYMENT_IDS {
        return Err(crate::http_response::bad_request(format!(
            "At most {} deployment_ids can be looked up at once",
            MAX_HISTORY_DEPLOYMENT_IDS
        )));
    }
    Ok(ids)
}

/// Keeps only the records of these deployments. The index still reads every record of the
/// account and region, but a page then carries a few small items instead of up to 1MB of whole
/// deployments, which is what made looking a few up in a long history slow.
fn narrow_to_deployment_ids(query: &mut Value, deployment_ids: &[String]) {
    if deployment_ids.is_empty() {
        return;
    }
    let placeholders: Vec<String> = (0..deployment_ids.len())
        .map(|i| format!(":history_deployment_id{}", i))
        .collect();
    let condition = format!("deployment_id IN ({})", placeholders.join(", "));
    query["FilterExpression"] = match query.get("FilterExpression").and_then(|v| v.as_str()) {
        Some(existing) => serde_json::json!(format!("({}) AND {}", existing, condition)),
        None => serde_json::json!(condition),
    };
    if !query
        .get("ExpressionAttributeValues")
        .is_some_and(|v| v.is_object())
    {
        query["ExpressionAttributeValues"] = serde_json::json!({});
    }
    if let Some(values) = query["ExpressionAttributeValues"].as_object_mut() {
        for (placeholder, id) in placeholders.iter().zip(deployment_ids) {
            values.insert(placeholder.clone(), serde_json::json!(id));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// Three pages, the middle one empty, each with the token of the next.
    struct PagingDb {
        tokens_seen: Mutex<Vec<Option<String>>>,
    }

    impl DatabaseQuery for PagingDb {
        async fn query_table(&self, _: &str, _: &Value, _: Option<&str>) -> Result<Value> {
            unreachable!("history is read in the workload account")
        }

        async fn query_workload_account(
            &self,
            _: &str,
            _: &str,
            query: &Value,
            _: Option<&str>,
        ) -> Result<Value> {
            let start = query
                .get("ExclusiveStartKey")
                .and_then(|k| k.get("page"))
                .and_then(|p| p.as_u64());
            self.tokens_seen
                .lock()
                .unwrap()
                .push(start.map(|p| p.to_string()));
            let token = |page: u64| {
                general_purpose::STANDARD.encode(serde_json::json!({ "page": page }).to_string())
            };
            Ok(match start {
                None => {
                    serde_json::json!({ "Items": [{ "deployment_id": "a/x", "epoch": 1 }], "next_token": token(1) })
                }
                Some(1) => serde_json::json!({ "Items": [], "next_token": token(2) }),
                _ => serde_json::json!({ "Items": [{ "deployment_id": "b/y", "epoch": 2 }] }),
            })
        }
    }

    #[tokio::test]
    async fn narrowed_history_is_followed_to_the_end_in_one_response() {
        let db = PagingDb {
            tokens_seen: Mutex::new(Vec::new()),
        };
        let payload = serde_json::json!({
            "project": "123456789012",
            "region": "us-west-2",
            "type": "deleted",
            "deployment_ids": "a/x,b/y",
        });

        let result = get_deployment_history_impl(
            &db,
            &payload,
            |_, _, _| serde_json::json!({}),
            |_, _, _| serde_json::json!({ "FilterExpression": "SK = :metadata" }),
        )
        .await
        .unwrap();

        let ids: Vec<&str> = result["Items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|i| i["deployment_id"].as_str().unwrap())
            .collect();
        assert_eq!(ids, ["b/y", "a/x"]);
        assert!(result.get("next_token").is_none());
        assert_eq!(
            *db.tokens_seen.lock().unwrap(),
            vec![None, Some("1".to_string()), Some("2".to_string())]
        );
    }

    #[test]
    fn history_can_be_narrowed_to_deployment_ids() {
        use serde_json::json;
        let mut query = json!({
            "FilterExpression": "SK = :metadata",
            "ExpressionAttributeValues": { ":metadata": "METADATA" },
        });
        let ids = history_deployment_ids(&json!({ "deployment_ids": "a/x, b/y,,a/x" })).unwrap();
        narrow_to_deployment_ids(&mut query, &ids);

        assert_eq!(
            query["FilterExpression"],
            json!("(SK = :metadata) AND deployment_id IN (:history_deployment_id0, :history_deployment_id1)")
        );
        assert_eq!(
            query["ExpressionAttributeValues"][":history_deployment_id1"],
            json!("b/y")
        );
        assert_eq!(
            query["ExpressionAttributeValues"][":metadata"],
            json!("METADATA")
        );

        let mut untouched = json!({ "KeyConditionExpression": "x = :x" });
        narrow_to_deployment_ids(&mut untouched, &[]);
        assert!(untouched.get("FilterExpression").is_none());

        let too_many = (0..101)
            .map(|i| i.to_string())
            .collect::<Vec<_>>()
            .join(",");
        assert!(history_deployment_ids(&json!({ "deployment_ids": too_many })).is_err());
    }

    #[derive(Default)]
    struct RecordingDb {
        calls: Mutex<Vec<Option<String>>>,
    }

    impl RecordingDb {
        fn calls(&self) -> Vec<Option<String>> {
            self.calls.lock().unwrap().clone()
        }
    }

    impl DatabaseQuery for RecordingDb {
        async fn query_table(
            &self,
            _container: &str,
            _query: &Value,
            _region: Option<&str>,
        ) -> Result<Value> {
            self.calls.lock().unwrap().push(None);
            Ok(serde_json::json!({
                "Items": [],
                "Count": 0
            }))
        }

        async fn query_workload_account(
            &self,
            workload_account: &str,
            _container: &str,
            _query: &Value,
            _region: Option<&str>,
        ) -> Result<Value> {
            self.calls
                .lock()
                .unwrap()
                .push(Some(workload_account.to_string()));
            Ok(serde_json::json!({
                "Items": [],
                "Count": 0
            }))
        }
    }

    #[tokio::test]
    async fn deployment_list_scopes_single_project_query() {
        let db = RecordingDb::default();
        let payload = serde_json::json!({
            "project": "111111111111",
            "region": "us-west-2"
        });

        get_deployments_impl(&db, &payload, |project, region, _, _| {
            serde_json::json!({
                "project": project,
                "region": region
            })
        })
        .await
        .unwrap();

        assert_eq!(db.calls(), vec![Some("111111111111".to_string())]);
    }

    #[tokio::test]
    async fn deployment_list_scopes_each_project_query() {
        let db = RecordingDb::default();
        let payload = serde_json::json!({
            "projects": ["111111111111", "222222222222"],
            "region": "us-west-2"
        });

        get_deployments_impl(&db, &payload, |project, region, _, _| {
            serde_json::json!({
                "project": project,
                "region": region
            })
        })
        .await
        .unwrap();

        assert_eq!(
            db.calls(),
            vec![
                Some("111111111111".to_string()),
                Some("222222222222".to_string())
            ]
        );
    }

    #[tokio::test]
    async fn events_query_scopes_project_query() {
        let db = RecordingDb::default();
        let payload = serde_json::json!({
            "project": "111111111111",
            "region": "us-west-2",
            "deployment_id": "module/name",
            "environment": "dev"
        });

        get_events_impl(
            &db,
            &payload,
            |project, region, deployment_id, environment, _| {
                serde_json::json!({
                    "project": project,
                    "region": region,
                    "deployment_id": deployment_id,
                    "environment": environment
                })
            },
        )
        .await
        .unwrap();

        assert_eq!(db.calls(), vec![Some("111111111111".to_string())]);
    }

    #[tokio::test]
    async fn change_records_query_scopes_project_query() {
        let db = RecordingDb::default();
        let payload = serde_json::json!({
            "project": "111111111111",
            "region": "us-west-2",
            "deployment_id": "module/name",
            "environment": "dev",
            "change_type": "mutate"
        });

        get_change_records_impl(
            &db,
            &payload,
            |project, region, environment, deployment_id, change_type| {
                serde_json::json!({
                    "project": project,
                    "region": region,
                    "environment": environment,
                    "deployment_id": deployment_id,
                    "change_type": change_type
                })
            },
        )
        .await
        .unwrap();

        // MUTATE also queries the legacy APPLY/DESTROY prefixes, all workload-scoped.
        assert_eq!(
            db.calls(),
            vec![
                Some("111111111111".to_string()),
                Some("111111111111".to_string()),
                Some("111111111111".to_string()),
            ]
        );
    }

    /// Mock DB that records the `change_type` prefix each query used and returns
    /// canned items keyed by that prefix, so merge/fallback behavior is testable.
    #[derive(Default)]
    struct PrefixDb {
        prefixes: Mutex<Vec<String>>,
        items_by_prefix: std::collections::HashMap<String, Vec<Value>>,
    }

    impl PrefixDb {
        fn queried_prefixes(&self) -> Vec<String> {
            self.prefixes.lock().unwrap().clone()
        }

        fn record(&self, query: &Value) {
            let prefix = query
                .get("change_type")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            self.prefixes.lock().unwrap().push(prefix);
        }

        fn response_for(&self, query: &Value) -> Value {
            let prefix = query
                .get("change_type")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let items = self
                .items_by_prefix
                .get(prefix)
                .cloned()
                .unwrap_or_default();
            let count = items.len();
            serde_json::json!({ "Items": items, "Count": count })
        }
    }

    impl DatabaseQuery for PrefixDb {
        async fn query_table(
            &self,
            _container: &str,
            query: &Value,
            _region: Option<&str>,
        ) -> Result<Value> {
            self.record(query);
            Ok(self.response_for(query))
        }

        async fn query_workload_account(
            &self,
            _workload_account: &str,
            _container: &str,
            query: &Value,
            _region: Option<&str>,
        ) -> Result<Value> {
            self.record(query);
            Ok(self.response_for(query))
        }
    }

    fn change_records_qb(
        _project: &str,
        _region: &str,
        _environment: &str,
        _deployment_id: &str,
        change_type: &str,
    ) -> Value {
        serde_json::json!({ "change_type": change_type })
    }

    fn record_with_epoch(job_id: &str, epoch: u64) -> Value {
        serde_json::json!({ "job_id": job_id, "epoch": epoch })
    }

    #[tokio::test]
    async fn change_records_mutate_merges_legacy_prefixes_sorted_by_epoch() {
        let mut items_by_prefix = std::collections::HashMap::new();
        items_by_prefix.insert("MUTATE".to_string(), vec![record_with_epoch("m1", 30)]);
        items_by_prefix.insert("APPLY".to_string(), vec![record_with_epoch("a1", 10)]);
        items_by_prefix.insert("DESTROY".to_string(), vec![record_with_epoch("d1", 20)]);
        let db = PrefixDb {
            items_by_prefix,
            ..Default::default()
        };
        let payload = serde_json::json!({
            "project": "111111111111",
            "region": "us-west-2",
            "deployment_id": "module/name",
            "environment": "dev",
            "change_type": "mutate"
        });

        let result = get_change_records_impl(&db, &payload, change_records_qb)
            .await
            .unwrap();

        assert_eq!(
            db.queried_prefixes(),
            vec!["MUTATE", "APPLY", "DESTROY"],
            "MUTATE should query itself plus the legacy prefixes"
        );

        let items = result.get("Items").and_then(|v| v.as_array()).unwrap();
        let job_ids: Vec<&str> = items
            .iter()
            .map(|i| i.get("job_id").and_then(|v| v.as_str()).unwrap())
            .collect();
        // Descending epoch: m1(30), d1(20), a1(10)
        assert_eq!(job_ids, vec!["m1", "d1", "a1"]);
        assert_eq!(result.get("Count").and_then(|v| v.as_u64()), Some(3));
    }

    #[tokio::test]
    async fn change_record_single_falls_back_to_legacy_prefix() {
        // Only a legacy APPLY record exists; MUTATE lookup must fall through to it.
        let mut items_by_prefix = std::collections::HashMap::new();
        items_by_prefix.insert("APPLY".to_string(), vec![record_with_epoch("job-123", 10)]);
        let db = PrefixDb {
            items_by_prefix,
            ..Default::default()
        };
        let payload = serde_json::json!({
            "project": "111111111111",
            "region": "us-west-2",
            "deployment_id": "module/name",
            "environment": "dev",
            "job_id": "job-123",
            "change_type": "mutate"
        });

        let result = get_change_record_impl(
            &db,
            &payload,
            |_project, _region, _environment, _deployment_id, _job_id, change_type| {
                serde_json::json!({ "change_type": change_type })
            },
        )
        .await
        .unwrap();

        // Stops at APPLY once found; DESTROY is never queried.
        assert_eq!(db.queried_prefixes(), vec!["MUTATE", "APPLY"]);
        assert_eq!(
            result.get("job_id").and_then(|v| v.as_str()),
            Some("job-123")
        );
    }

    #[tokio::test]
    async fn modules_query_does_not_use_workload_scope() {
        let db = RecordingDb::default();
        let payload = serde_json::json!({});

        get_modules_impl(&db, &payload, |_, _, _| {
            serde_json::json!({
                "KeyConditionExpression": "PK = :pk",
                "ExpressionAttributeValues": {
                    ":pk": "MODULES"
                }
            })
        })
        .await
        .unwrap();

        assert_eq!(db.calls(), vec![None]);
    }

    #[tokio::test]
    async fn projects_query_does_not_use_workload_scope() {
        let db = RecordingDb::default();
        let payload = serde_json::json!({});

        get_projects_impl(&db, &payload, || {
            serde_json::json!({
                "KeyConditionExpression": "PK = :pk",
                "ExpressionAttributeValues": {
                    ":pk": "PROJECTS"
                }
            })
        })
        .await
        .unwrap();

        assert_eq!(db.calls(), vec![None]);
    }

    /// Mock DB that reports the most queries it ever had in flight at once.
    #[derive(Default)]
    struct ConcurrencyDb {
        in_flight: std::sync::atomic::AtomicUsize,
        peak: std::sync::atomic::AtomicUsize,
    }

    impl ConcurrencyDb {
        fn peak(&self) -> usize {
            self.peak.load(std::sync::atomic::Ordering::SeqCst)
        }
    }

    impl DatabaseQuery for ConcurrencyDb {
        async fn query_table(
            &self,
            _container: &str,
            _query: &Value,
            _region: Option<&str>,
        ) -> Result<Value> {
            Ok(serde_json::json!({ "Items": [], "Count": 0 }))
        }

        async fn query_workload_account(
            &self,
            _workload_account: &str,
            _container: &str,
            _query: &Value,
            _region: Option<&str>,
        ) -> Result<Value> {
            use std::sync::atomic::Ordering;
            let current = self.in_flight.fetch_add(1, Ordering::SeqCst) + 1;
            self.peak.fetch_max(current, Ordering::SeqCst);
            // Let the other queries start before this one finishes; without a pause
            // here every query would complete before the next began and an unbounded
            // fan-out would look identical to a bounded one.
            for _ in 0..4 {
                tokio::task::yield_now().await;
            }
            self.in_flight.fetch_sub(1, Ordering::SeqCst);
            Ok(serde_json::json!({ "Items": [], "Count": 0 }))
        }
    }

    /// Mock DB that fails for the nominated projects and answers for the rest.
    struct FlakyDb {
        failing_projects: &'static [&'static str],
    }

    impl DatabaseQuery for FlakyDb {
        async fn query_table(
            &self,
            _container: &str,
            _query: &Value,
            _region: Option<&str>,
        ) -> Result<Value> {
            Ok(serde_json::json!({ "Items": [], "Count": 0 }))
        }

        async fn query_workload_account(
            &self,
            workload_account: &str,
            _container: &str,
            _query: &Value,
            _region: Option<&str>,
        ) -> Result<Value> {
            if self.failing_projects.contains(&workload_account) {
                return Err(anyhow!("simulated outage for {}", workload_account));
            }
            Ok(serde_json::json!({
                "Items": [{ "project_id": workload_account }],
                "Count": 1
            }))
        }
    }

    /// Mock DB that answers every query with the same items and keeps the queries.
    struct StaticDb {
        items: Vec<Value>,
        queries: Mutex<Vec<Value>>,
    }

    impl StaticDb {
        fn new(items: Vec<Value>) -> Self {
            Self {
                items,
                queries: Mutex::new(Vec::new()),
            }
        }

        fn queries(&self) -> Vec<Value> {
            self.queries.lock().unwrap().clone()
        }
    }

    impl DatabaseQuery for StaticDb {
        async fn query_table(
            &self,
            _container: &str,
            query: &Value,
            _region: Option<&str>,
        ) -> Result<Value> {
            self.queries.lock().unwrap().push(query.clone());
            Ok(serde_json::json!({
                "Items": self.items,
                "Count": self.items.len()
            }))
        }
    }

    #[tokio::test]
    async fn deployment_list_paces_the_fan_out_over_many_projects() {
        let db = ConcurrencyDb::default();
        let projects: Vec<String> = (0..25).map(|i| format!("{:012}", i)).collect();
        let payload = serde_json::json!({
            "projects": projects,
            "region": "us-west-2"
        });

        get_deployments_impl(
            &db,
            &payload,
            |project, region, _, _| serde_json::json!({ "project": project, "region": region }),
        )
        .await
        .unwrap();

        assert_eq!(db.peak(), MAX_CONCURRENT_WORKLOAD_QUERIES);
    }

    #[tokio::test]
    async fn deployment_list_keeps_the_projects_that_answered() {
        let db = FlakyDb {
            failing_projects: &["222222222222"],
        };
        let payload = serde_json::json!({
            "projects": ["111111111111", "222222222222", "333333333333"],
            "region": "us-west-2"
        });

        let result = get_deployments_impl(
            &db,
            &payload,
            |project, region, _, _| serde_json::json!({ "project": project, "region": region }),
        )
        .await
        .unwrap();

        let mut projects: Vec<&str> = result["Items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|item| item["project_id"].as_str().unwrap())
            .collect();
        projects.sort();

        assert_eq!(projects, vec!["111111111111", "333333333333"]);
        assert_eq!(result["Count"], 2);
        assert_eq!(result["partial_failures"], 1);
    }

    #[tokio::test]
    async fn deployment_list_fails_when_no_project_answered() {
        let db = FlakyDb {
            failing_projects: &["111111111111", "222222222222"],
        };
        let payload = serde_json::json!({
            "projects": ["111111111111", "222222222222"],
            "region": "us-west-2"
        });

        let error = get_deployments_impl(
            &db,
            &payload,
            |project, region, _, _| serde_json::json!({ "project": project, "region": region }),
        )
        .await
        .unwrap_err();

        assert!(
            error.to_string().contains("failed for all 2 projects"),
            "unexpected error: {error}"
        );
    }

    #[tokio::test]
    async fn module_deployments_drop_projects_outside_the_callers_grants() {
        let db = StaticDb::new(vec![
            serde_json::json!({ "deployment_id": "kept", "project_id": "111111111111" }),
            serde_json::json!({ "deployment_id": "hidden", "project_id": "222222222222" }),
        ]);
        let payload = serde_json::json!({
            "module": "s3bucket",
            "region": "us-west-2",
            "allowed_projects": ["111111111111"]
        });

        let result = get_deployments_for_module_globally_impl(
            &db,
            &payload,
            |module, region, _| serde_json::json!({ "module": module, "region": region }),
        )
        .await
        .unwrap();

        let items = result["Items"].as_array().unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["deployment_id"], "kept");
        assert_eq!(result["Count"], 1);
    }

    /// Serves `pages` DynamoDB pages of one row each, chaining them by LastEvaluatedKey.
    struct PagedDb {
        pages: usize,
        reads: Mutex<usize>,
    }

    impl DatabaseQuery for PagedDb {
        async fn query_table(
            &self,
            _container: &str,
            query: &Value,
            _region: Option<&str>,
        ) -> Result<Value> {
            *self.reads.lock().unwrap() += 1;
            let index = query
                .get("ExclusiveStartKey")
                .and_then(|k| k["page"].as_u64())
                .map_or(0, |p| p as usize + 1);
            // Alternate projects so the grant filter has something to drop.
            let project = if index % 2 == 0 {
                "111111111111"
            } else {
                "222222222222"
            };
            let mut response = serde_json::json!({
                "Items": [{ "deployment_id": format!("d{index}"), "project_id": project }],
                "Count": 1
            });
            if index + 1 < self.pages {
                response["LastEvaluatedKey"] = serde_json::json!({ "page": index });
                response["next_token"] = serde_json::json!(format!("token-{index}"));
            }
            Ok(response)
        }
    }

    #[tokio::test]
    async fn fleet_deployments_follow_several_pages_per_request() {
        let db = PagedDb {
            pages: 3,
            reads: Mutex::new(0),
        };
        let payload = serde_json::json!({ "region": "us-west-2" });

        let result = get_fleet_deployments_impl(
            &db,
            &payload,
            |region, _| serde_json::json!({ "region": region }),
        )
        .await
        .unwrap();

        assert_eq!(*db.reads.lock().unwrap(), 3);
        assert_eq!(result["Count"], 3);
        // All of it fit in one request, so there is nothing to resume.
        assert!(result.get("next_token").is_none());
    }

    #[tokio::test]
    async fn fleet_deployments_hand_back_a_token_at_the_page_cap() {
        let db = PagedDb {
            pages: MAX_PAGES_PER_REQUEST + 5,
            reads: Mutex::new(0),
        };
        let payload = serde_json::json!({ "region": "us-west-2" });

        let result = get_fleet_deployments_impl(
            &db,
            &payload,
            |region, _| serde_json::json!({ "region": region }),
        )
        .await
        .unwrap();

        assert_eq!(*db.reads.lock().unwrap(), MAX_PAGES_PER_REQUEST);
        // The token of the last page read, so the next request picks up right after it.
        assert_eq!(
            result["next_token"],
            format!("token-{}", MAX_PAGES_PER_REQUEST - 1)
        );
    }

    #[tokio::test]
    async fn fleet_deployments_read_one_page_when_a_limit_is_given() {
        let db = PagedDb {
            pages: 3,
            reads: Mutex::new(0),
        };
        let payload = serde_json::json!({ "region": "us-west-2", "limit": 1 });

        let result = get_fleet_deployments_impl(
            &db,
            &payload,
            |region, _| serde_json::json!({ "region": region }),
        )
        .await
        .unwrap();

        assert_eq!(*db.reads.lock().unwrap(), 1);
        assert_eq!(result["next_token"], "token-0");
    }

    #[tokio::test]
    async fn fleet_deployments_drop_projects_outside_the_callers_grants_on_every_page() {
        let db = PagedDb {
            pages: 4,
            reads: Mutex::new(0),
        };
        let payload = serde_json::json!({
            "region": "us-west-2",
            "user_id": "someone",
            "allowed_projects": ["111111111111"]
        });

        let result = get_fleet_deployments_impl(
            &db,
            &payload,
            |region, _| serde_json::json!({ "region": region }),
        )
        .await
        .unwrap();

        let ids: Vec<&str> = result["Items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|item| item["deployment_id"].as_str().unwrap())
            .collect();
        assert_eq!(ids, vec!["d0", "d2"]);
        assert_eq!(result["Count"], 2);
    }

    #[tokio::test]
    async fn fleet_deployments_show_nothing_to_a_user_whose_token_grants_nothing() {
        let db = PagedDb {
            pages: 2,
            reads: Mutex::new(0),
        };
        let payload = serde_json::json!({ "region": "us-west-2", "user_id": "someone" });

        let result = get_fleet_deployments_impl(
            &db,
            &payload,
            |region, _| serde_json::json!({ "region": region }),
        )
        .await
        .unwrap();

        assert_eq!(result["Count"], 0);
        assert_eq!(*db.reads.lock().unwrap(), 0);
    }

    /// A token that carries no grants has to mean "nothing", not "no filter" — the same
    /// reading `handlers::get_projects` gives it. Getting this backwards on this endpoint
    /// hands back every project in the organisation, since no IAM scoping stands behind
    /// the global index.
    #[tokio::test]
    async fn module_deployments_show_nothing_to_a_user_whose_token_grants_nothing() {
        let db = StaticDb::new(vec![
            serde_json::json!({ "deployment_id": "any", "project_id": "111111111111" }),
        ]);
        let payload = serde_json::json!({
            "module": "s3bucket",
            "region": "us-west-2",
            "user_id": "someone@example.com"
        });

        let result = get_deployments_for_module_globally_impl(
            &db,
            &payload,
            |module, region, _| serde_json::json!({ "module": module, "region": region }),
        )
        .await
        .unwrap();

        assert_eq!(result["Items"].as_array().unwrap().len(), 0);
        assert_eq!(result["Count"], 0);
        assert!(
            db.queries().is_empty(),
            "a request that can see nothing must not reach the database"
        );
    }

    /// No user context at all is the unauthenticated local build, where `get_projects`
    /// likewise returns everything. Production cannot reach this: the route answers 401
    /// before the handler runs.
    #[tokio::test]
    async fn module_deployments_are_unfiltered_when_no_user_context_is_attached() {
        let db = StaticDb::new(vec![
            serde_json::json!({ "deployment_id": "a", "project_id": "111111111111" }),
            serde_json::json!({ "deployment_id": "b", "project_id": "222222222222" }),
        ]);
        let payload = serde_json::json!({
            "module": "s3bucket",
            "region": "us-west-2"
        });

        let result = get_deployments_for_module_globally_impl(
            &db,
            &payload,
            |module, region, _| serde_json::json!({ "module": module, "region": region }),
        )
        .await
        .unwrap();

        assert_eq!(result["Items"].as_array().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn module_deployments_return_nothing_for_a_caller_granted_nothing() {
        let db = StaticDb::new(vec![
            serde_json::json!({ "deployment_id": "any", "project_id": "111111111111" }),
        ]);
        let payload = serde_json::json!({
            "module": "s3bucket",
            "region": "us-west-2",
            "allowed_projects": []
        });

        let result = get_deployments_for_module_globally_impl(
            &db,
            &payload,
            |module, region, _| serde_json::json!({ "module": module, "region": region }),
        )
        .await
        .unwrap();

        assert_eq!(result["Items"].as_array().unwrap().len(), 0);
        assert_eq!(result["Count"], 0);
        assert!(
            db.queries().is_empty(),
            "a caller granted nothing needs no query"
        );
    }
}
