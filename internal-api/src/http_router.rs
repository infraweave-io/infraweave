use axum::{
    extract::{Path, Query},
    http::{HeaderMap, Method, StatusCode},
    middleware,
    response::{IntoResponse, Json},
    routing::{get, post, put},
    Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use tower_http::cors::{Any, CorsLayer};

use crate::deployment_routes::{
    apply_deployment_from_body, destroy_deployment_from_body, plan_deployment_from_body,
    reapply_deployment_from_body,
};
use crate::handlers;
use crate::http_authz::{auth_middleware, extract_jwt_claims};
use crate::http_response::handle_result;
use crate::publish_routes::{
    deprecate_module, deprecate_stack, download_provider, publish_module, publish_policy,
    publish_provider, publish_stack,
};

pub fn create_router() -> Router {
    // Configure CORS to allow requests from any origin
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods([
            Method::GET,
            Method::POST,
            Method::PUT,
            Method::DELETE,
            Method::OPTIONS,
        ])
        .allow_headers(Any)
        .allow_credentials(false);

    // Routes that require project-level authorization
    let protected_routes = Router::new()
        .route(
            "/api/v1/deployment/{project}/{region}/{*rest}",
            get(describe_deployment),
        )
        // Multi-project deployments list supported via comma-separated project param
        .route(
            "/api/v1/deployments/{project}/{region}",
            get(get_deployments),
        )
        .route(
            "/api/v1/deployments/module/{project}/{region}/{module}",
            get(get_deployments_for_module),
        )
        .route(
            "/api/v1/deployments/history/{project}/{region}",
            get(get_deployments_history),
        )
        // Specific endpoint for deployment plan status by job_id
        .route(
            "/api/v1/plan/{project}/{region}/{*rest}",
            get(describe_plan_deployment),
        )
        .route("/api/v1/logs/{project}/{region}/{job_id}", get(read_logs))
        .route("/api/v1/events/{project}/{region}/{*rest}", get(get_events))
        .route(
            "/api/v1/change_records/{project}/{region}/{*rest}",
            get(get_change_records),
        )
        .route(
            "/api/v1/change_record/{project}/{region}/{*rest}",
            get(get_change_record),
        )
        .route(
            "/api/v1/change_record_graph/{project}/{region}/{*rest}",
            get(get_change_record_graph),
        )
        .route(
            "/api/v1/deployment_graph/{project}/{region}/{*rest}",
            get(get_deployment_graph),
        )
        // Provider download route - returns base64 content (requires auth)
        .route("/api/v1/provider/download", post(download_provider))
        // Plan/Apply/Destroy operations
        .route("/api/v1/apply", post(apply_deployment_from_body))
        .route("/api/v1/plan", post(plan_deployment_from_body))
        .route("/api/v1/destroy", post(destroy_deployment_from_body))
        .route("/api/v1/reapply", post(reapply_deployment_from_body))
        // Job status route - use wildcard to handle ARNs with slashes
        .route(
            "/api/v1/job_status/{project}/{region}/{*rest}",
            get(get_job_status_http),
        )
        .layer(middleware::from_fn(auth_middleware));

    // Open routes / Global lookups
    let open_routes = Router::new()
        .route(
            "/2015-03-31/functions/{function_name}/invocations",
            post(handlers::handle_lambda_invocation),
        )
        // Authentication / Token bridge route (generic OIDC)
        .route("/api/v1/auth/token", post(handle_auth_token))
        // Meta endpoint for region discovery
        // MUST be unauthenticated to allow clients to discover region via Latency Based Routing
        // before they can sign requests with the correct region.
        .route("/api/v1/meta", get(get_meta_info))
        .route("/api/v1/modules", get(get_modules))
        .route("/api/v1/projects", get(get_projects))
        .route("/api/v1/stacks", get(get_stacks))
        .route("/api/v1/providers", get(get_providers))
        .route(
            "/api/v1/module/{track}/{module_name}/{module_version}",
            get(get_module_version),
        )
        .route(
            "/api/v1/module/{track}/{module_name}/{module_version}/download",
            get(get_module_download_url),
        )
        .route(
            "/api/v1/stack/{track}/{stack_name}/{stack_version}",
            get(get_stack_version),
        )
        .route(
            "/api/v1/stack/{track}/{stack_name}/{stack_version}/download",
            get(get_stack_download_url),
        )
        .route(
            "/api/v1/modules/versions/{track}/{module}",
            get(get_all_versions_for_module),
        )
        .route(
            "/api/v1/stacks/versions/{track}/{stack}",
            get(get_all_versions_for_stack),
        )
        .route(
            "/api/v1/provider/{track}/{provider}/{version}",
            get(get_provider_version),
        )
        .route(
            "/api/v1/provider/{track}/{provider}/{version}/download",
            get(get_provider_download_url),
        )
        // Policy routes
        .route("/api/v1/policies/{environment}", get(get_policies))
        .route(
            "/api/v1/policy/{environment}/{policy_name}/{policy_version}",
            get(get_policy_version),
        );

    // Routes that require publish permission.
    let publish_protected_routes = Router::new()
        // Module deprecation route
        .route(
            "/api/v1/module/{track}/{module}/{version}/deprecate",
            put(deprecate_module),
        )
        // Stack deprecation route
        .route(
            "/api/v1/stack/{track}/{stack}/{version}/deprecate",
            put(deprecate_stack),
        )
        // Module publish route - accepts pre-built modules
        .route("/api/v1/module/publish", post(publish_module))
        // Stack publish route - accepts pre-built stacks (same format as modules)
        .route("/api/v1/stack/publish", post(publish_stack))
        // Provider publish route - accepts pre-built providers
        .route("/api/v1/provider/publish", post(publish_provider))
        // Policy publish route - accepts pre-built policies
        .route("/api/v1/policy/publish", post(publish_policy));

    open_routes
        .merge(protected_routes)
        .merge(publish_protected_routes)
        // Add CORS layer
        .layer(cors)
    // NOTE: CompressionLayer removed because API Gateway v2 HTTP API strips the
    // Content-Encoding header, causing clients to receive compressed data without
    // knowing it's compressed. Use CloudFront for compression instead.
}

// Handler implementations

async fn describe_plan_deployment(
    Path((project, region, rest)): Path<(String, String, String)>,
) -> impl IntoResponse {
    // Expected format: environment1/environment2/deployment1/deployment2/job_id
    // But since environment/deployment can contain slashes, we need to be careful
    // However, in api_infra logic it passes: deployment_id, environment, job_id
    // The previous http_describe_deployment expected env/dep
    // Let's adopt a convention: /api/v1/plan/{project}/{region}/{env}/{dep}/{job_id}
    // But env and dep can have slashes.

    // Safer to split by slash and take last segment as job_id
    let parts: Vec<&str> = rest.split('/').collect();
    if parts.len() < 3 {
        // minimal: env/dep/job_id (Assuming env and dep are at least 1 segment)
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": format!("Invalid path format. Expected .../environment/deployment/job_id, got {}", rest)
            })),
        )
            .into_response();
    }

    let job_id = parts.last().unwrap().to_string();

    // The rest before job_id is env+deployment.
    // We kow from describe_deployment:
    // environment = parts[0]/parts[1]
    // deployment_id = parts[2]/parts[3]
    // And here we add job_id as parts[4]

    // Let's assume the standard 2-segment structure if possible, but match what describe_deployment does
    if parts.len() != 5 {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": format!("Invalid path format. Expected exactly 5 segments (env1/env2/dep1/dep2/job_id), got {}", parts.len())
            })),
        )
            .into_response();
    }

    let environment = format!("{}/{}", parts[0], parts[1]);
    let deployment_id = format!("{}/{}", parts[2], parts[3]);

    handle_result(
        handlers::describe_plan_deployment(&json!({
            "project": project,
            "region": region,
            "environment": environment,
            "deployment_id": deployment_id,
            "job_id": job_id
        }))
        .await,
    )
    .await
    .into_response()
}

async fn describe_deployment(
    Path((project, region, rest)): Path<(String, String, String)>,
) -> impl IntoResponse {
    // Middleware handles auth check

    // Parse the rest parameter to extract environment and deployment_id
    // Expected format: environment1/environment2/deployment1/deployment2
    let parts: Vec<&str> = rest.split('/').collect();

    if parts.len() != 4 {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": format!("Invalid path format. Expected exactly 4 segments (env1/env2/dep1/dep2), got {}", parts.len())
            })),
        )
            .into_response();
    }

    let environment = format!("{}/{}", parts[0], parts[1]);
    let deployment_id = format!("{}/{}", parts[2], parts[3]);

    handle_result(
        handlers::describe_deployment(&json!({
            "project": project,
            "region": region,
            "environment": environment,
            "deployment_id": deployment_id
        }))
        .await,
    )
    .await
    .into_response()
}

async fn get_deployments(
    Path((project, region)): Path<(String, String)>,
    Query(query): Query<PaginationQuery>,
) -> impl IntoResponse {
    let project_list: Vec<&str> = project
        .split(',')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .collect();

    let mut payload = json!({
        "region": region
    });

    // Support comma-separated projects in the path
    if project_list.len() > 1 {
        payload["projects"] = json!(project_list);
    } else {
        payload["project"] = json!(project);
    }

    if let Some(limit) = query.limit {
        payload["limit"] = json!(limit);
    }
    if let Some(next_token) = query.next_token {
        payload["next_token"] = json!(next_token);
    }

    handle_result(handlers::get_deployments(&payload).await)
        .await
        .into_response()
}

async fn get_deployments_for_module(
    Path((project, region, module)): Path<(String, String, String)>,
    Query(query): Query<PaginationQuery>,
) -> impl IntoResponse {
    let mut payload = json!({
        "region": region,
        "module": module
    });

    let project_list: Vec<&str> = project.split(',').collect();
    if project_list.len() > 1 {
        payload["projects"] = json!(project_list);
    } else {
        payload["project"] = json!(project);
    }

    if let Some(limit) = query.limit {
        payload["limit"] = json!(limit);
    }
    if let Some(next_token) = query.next_token {
        payload["next_token"] = json!(next_token);
    }

    handle_result(handlers::get_deployments_for_module(&payload).await)
        .await
        .into_response()
}

async fn get_deployments_history(
    Path((project, region)): Path<(String, String)>,
    Query(query): Query<DeploymentHistoryQuery>,
) -> impl IntoResponse {
    let mut payload = json!({
        "project": project,
        "region": region
    });

    if let Some(environment) = query.environment {
        payload["environment"] = json!(environment);
    }

    payload["type"] = json!(query.r#type);

    if let Some(limit) = query.limit {
        payload["limit"] = json!(limit);
    }
    if let Some(next_token) = query.next_token {
        payload["next_token"] = json!(next_token);
    }

    handle_result(handlers::get_deployment_history(&payload).await)
        .await
        .into_response()
}

#[derive(Deserialize)]
struct PaginationQuery {
    limit: Option<i64>,
    next_token: Option<String>,
}

#[derive(Deserialize)]
struct DeploymentHistoryQuery {
    limit: Option<i64>,
    next_token: Option<String>,
    environment: Option<String>,
    r#type: String, // "plans" or "deleted" (required)
}

#[derive(Deserialize)]
struct ModulePaginationQuery {
    limit: Option<i64>,
    next_token: Option<String>,
    #[serde(default)]
    include_deprecated: Option<bool>,
    #[serde(default)]
    include_dev000: Option<bool>,
}

#[derive(Deserialize)]
struct EventPaginationQuery {
    limit: Option<i64>,
    next_token: Option<String>,
    event_type: Option<String>,
}

#[derive(Deserialize)]
struct ChangeRecordPaginationQuery {
    limit: Option<i64>,
    next_token: Option<String>,
    change_type: String,
}

async fn read_logs(
    Path((project, region, job_id)): Path<(String, String, String)>,
    Query(query): Query<PaginationQuery>,
) -> impl IntoResponse {
    let mut data = json!({
        "project_id": project,
        "region": region,
        "job_id": job_id
    });

    if let Some(limit) = query.limit {
        data["limit"] = json!(limit);
    }
    if let Some(next_token) = query.next_token {
        data["next_token"] = json!(next_token);
    }

    handle_result(
        handlers::read_logs(&json!({
            "data": data
        }))
        .await,
    )
    .await
    .into_response()
}

async fn get_events(
    Path((project, region, rest)): Path<(String, String, String)>,
    Query(query): Query<EventPaginationQuery>,
) -> impl IntoResponse {
    // Parse the rest parameter to extract environment and deployment_id
    // Expected format: environment1/environment2/deployment1/deployment2
    let parts: Vec<&str> = rest.split('/').collect();

    log::info!(
        "get_events: rest='{}', parts={:?}, len={}",
        rest,
        parts,
        parts.len()
    );

    if parts.len() != 4 {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": format!("Invalid path format. Expected exactly 4 segments (env1/env2/dep1/dep2), got {}", parts.len())
            })),
        )
            .into_response();
    }

    let environment = format!("{}/{}", parts[0], parts[1]);
    let deployment_id = format!("{}/{}", parts[2], parts[3]);

    log::info!(
        "get_events: environment='{}', deployment_id='{}'",
        environment,
        deployment_id
    );

    let mut payload = json!({
        "project": project,
        "region": region,
        "environment": environment,
        "deployment_id": deployment_id
    });

    if let Some(limit) = query.limit {
        payload["limit"] = json!(limit);
    }
    if let Some(next_token) = query.next_token {
        payload["next_token"] = json!(next_token);
    }
    if let Some(event_type) = query.event_type {
        payload["event_type"] = json!(event_type);
    }

    handle_result(handlers::get_events(&payload).await)
        .await
        .into_response()
}

async fn get_change_records(
    Path((project, region, rest)): Path<(String, String, String)>,
    Query(query): Query<ChangeRecordPaginationQuery>,
) -> impl IntoResponse {
    // Expected format: environment1/environment2/deployment1/deployment2
    let parts: Vec<&str> = rest.split('/').collect();

    if parts.len() != 4 {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": format!("Invalid path format. Expected exactly 4 segments (env1/env2/dep1/dep2), got {}", parts.len())
            })),
        )
            .into_response();
    }

    let environment = format!("{}/{}", parts[0], parts[1]);
    let deployment_id = format!("{}/{}", parts[2], parts[3]);

    let mut payload = json!({
        "project": project,
        "region": region,
        "environment": environment,
        "deployment_id": deployment_id,
        "change_type": query.change_type
    });

    if let Some(limit) = query.limit {
        payload["limit"] = json!(limit);
    }
    if let Some(next_token) = query.next_token {
        payload["next_token"] = json!(next_token);
    }

    handle_result(handlers::get_change_records(&payload).await)
        .await
        .into_response()
}

async fn get_change_record(
    Path((project, region, rest)): Path<(String, String, String)>,
) -> impl IntoResponse {
    // Parse the rest parameter to extract environment, deployment_id, job_id, and change_type
    // Expected format: environment1/environment2/deployment1/deployment2/job_id/change_type
    let parts: Vec<&str> = rest.split('/').collect();

    if parts.len() != 6 {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": format!("Invalid path format. Expected exactly 6 segments (env1/env2/dep1/dep2/job_id/change_type), got {}", parts.len())
            })),
        )
            .into_response();
    }

    let environment = format!("{}/{}", parts[0], parts[1]);
    let deployment_id = format!("{}/{}", parts[2], parts[3]);
    let job_id = parts[4].to_string();
    let change_type = parts[5].to_string();

    handle_result(
        handlers::get_change_record(&json!({
            "project": project,
            "region": region,
            "environment": environment,
            "deployment_id": deployment_id,
            "job_id": job_id,
            "change_type": change_type
        }))
        .await,
    )
    .await
    .into_response()
}

async fn get_change_record_graph(
    Path((project, region, rest)): Path<(String, String, String)>,
) -> impl IntoResponse {
    // Parse the rest parameter to extract environment, deployment_id, job_id, and change_type
    // Expected format: environment1/environment2/deployment1/deployment2/job_id/change_type
    let parts: Vec<&str> = rest.split('/').collect();

    if parts.len() != 6 {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": format!("Invalid path format. Expected exactly 6 segments (env1/env2/dep1/dep2/job_id/change_type), got {}", parts.len())
            })),
        )
            .into_response();
    }

    let environment = format!("{}/{}", parts[0], parts[1]);
    let deployment_id = format!("{}/{}", parts[2], parts[3]);
    let job_id = parts[4].to_string();
    let change_type = parts[5].to_string();

    let result = handlers::get_change_record_graph(&json!({
        "project": project,
        "region": region,
        "environment": environment,
        "deployment_id": deployment_id,
        "job_id": job_id,
        "change_type": change_type
    }))
    .await;

    match result {
        Ok(response) => response,
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({
                "error": format!("{}", e)
            })),
        )
            .into_response(),
    }
}

async fn get_deployment_graph(
    Path((project, region, rest)): Path<(String, String, String)>,
    Query(params): Query<HashMap<String, String>>,
) -> impl IntoResponse {
    // Parse the rest parameter to extract environment, deployment_id
    // Expected format: environment1/environment2/deployment1/deployment2
    let parts: Vec<&str> = rest.split('/').collect();

    if parts.len() != 4 {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": format!("Invalid path format. Expected exactly 4 segments (env1/env2/dep1/dep2), got {}", parts.len())
            })),
        )
            .into_response();
    }

    let environment = format!("{}/{}", parts[0], parts[1]);
    let deployment_id = format!("{}/{}", parts[2], parts[3]);

    let mut payload = json!({
        "project": project,
        "region": region,
        "environment": environment,
        "deployment_id": deployment_id
    });

    // Merge query params into payload
    if let Some(obj) = payload.as_object_mut() {
        for (k, v) in params {
            obj.insert(k, Value::String(v));
        }
    }

    let result = handlers::get_deployment_graph(&payload).await;

    match result {
        Ok(response) => response,
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({
                "error": format!("{}", e)
            })),
        )
            .into_response(),
    }
}

async fn get_modules(Query(query): Query<ModulePaginationQuery>) -> impl IntoResponse {
    let mut payload = json!({});
    if let Some(limit) = query.limit {
        payload["limit"] = json!(limit);
    }
    if let Some(next_token) = query.next_token {
        payload["next_token"] = json!(next_token);
    }
    if let Some(include_deprecated) = query.include_deprecated {
        payload["include_deprecated"] = json!(include_deprecated);
    }
    if let Some(include_dev000) = query.include_dev000 {
        payload["include_dev000"] = json!(include_dev000);
    }
    handle_result(handlers::get_modules(&payload).await).await
}

async fn get_projects(
    headers: HeaderMap,
    Query(query): Query<PaginationQuery>,
) -> impl IntoResponse {
    let user_id = match headers.get("x-auth-user").and_then(|v| v.to_str().ok()) {
        Some(uid) => uid.to_string(),
        None => {
            #[cfg(feature = "local")]
            {
                log::warn!("Missing x-auth-user header, using 'local-user' (LOCAL MODE ONLY)");
                "local-user".to_string()
            }
            #[cfg(not(feature = "local"))]
            {
                return (
                    StatusCode::UNAUTHORIZED,
                    Json(json!({ "error": "Missing authentication user context" })),
                )
                    .into_response();
            }
        }
    };

    let mut payload = json!({
        "user_id": user_id
    });

    // Extract allowed_projects from JWT claims (configurable claim key)
    if let Some(claims) = extract_jwt_claims(&headers) {
        let claim_key = crate::auth_handler::allowed_projects_claim_key();
        if let Some(allowed_projects_str) = claims.get(&claim_key).and_then(|v| v.as_str()) {
            let allowed_projects: Vec<String> = allowed_projects_str
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();

            if !allowed_projects.is_empty() {
                log::info!(
                    "Applying {} allowed_projects from JWT claims",
                    allowed_projects.len()
                );
                payload["allowed_projects"] = json!(allowed_projects);
            }
        }
    }

    if let Some(limit) = query.limit {
        payload["limit"] = json!(limit);
    }
    if let Some(next_token) = query.next_token {
        payload["next_token"] = json!(next_token);
    }

    handle_result(handlers::get_projects(&payload).await)
        .await
        .into_response()
}

async fn get_stacks(Query(query): Query<ModulePaginationQuery>) -> impl IntoResponse {
    let mut payload = json!({});
    if let Some(limit) = query.limit {
        payload["limit"] = json!(limit);
    }
    if let Some(next_token) = query.next_token {
        payload["next_token"] = json!(next_token);
    }
    if let Some(include_deprecated) = query.include_deprecated {
        payload["include_deprecated"] = json!(include_deprecated);
    }
    if let Some(include_dev000) = query.include_dev000 {
        payload["include_dev000"] = json!(include_dev000);
    }
    handle_result(handlers::get_stacks(&payload).await).await
}

async fn get_providers(Query(query): Query<PaginationQuery>) -> impl IntoResponse {
    let mut payload = json!({});
    if let Some(limit) = query.limit {
        payload["limit"] = json!(limit);
    }
    if let Some(next_token) = query.next_token {
        payload["next_token"] = json!(next_token);
    }
    handle_result(handlers::get_providers(&payload).await).await
}

async fn get_policies(
    Path(environment): Path<String>,
    Query(query): Query<PaginationQuery>,
) -> impl IntoResponse {
    let mut payload = json!({
        "environment": environment
    });
    if let Some(limit) = query.limit {
        payload["limit"] = json!(limit);
    }
    if let Some(next_token) = query.next_token {
        payload["next_token"] = json!(next_token);
    }
    handle_result(handlers::get_policies(&payload).await).await
}

async fn get_policy_version(
    Path((environment, policy_name, policy_version)): Path<(String, String, String)>,
) -> impl IntoResponse {
    handle_result(
        handlers::get_policy_version(&json!({
            "environment": environment,
            "policy_name": policy_name,
            "policy_version": policy_version
        }))
        .await,
    )
    .await
}

async fn get_module_version(
    Path((track, module_name, module_version)): Path<(String, String, String)>,
) -> impl IntoResponse {
    handle_result(
        handlers::get_module_version(&json!({
            "track": track,
            "module_name": module_name,
            "module_version": module_version
        }))
        .await,
    )
    .await
}

async fn get_module_download_url(
    Path((track, module_name, module_version)): Path<(String, String, String)>,
) -> impl IntoResponse {
    let result = handlers::get_module_download_url(&json!({
        "track": track,
        "module_name": module_name,
        "module_version": module_version
    }))
    .await;

    match result {
        Ok(response) => response,
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({
                "error": format!("{}", e)
            })),
        )
            .into_response(),
    }
}

async fn get_stack_version(
    Path((track, stack_name, stack_version)): Path<(String, String, String)>,
) -> impl IntoResponse {
    handle_result(
        handlers::get_stack_version(&json!({
            "track": track,
            "stack_name": stack_name,
            "stack_version": stack_version
        }))
        .await,
    )
    .await
}

async fn get_stack_download_url(
    Path((track, stack_name, stack_version)): Path<(String, String, String)>,
) -> impl IntoResponse {
    let result = handlers::get_stack_download_url(&json!({
        "track": track,
        "stack_name": stack_name,
        "stack_version": stack_version
    }))
    .await;

    match result {
        Ok(response) => response,
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({
                "error": format!("{}", e)
            })),
        )
            .into_response(),
    }
}

async fn get_all_versions_for_module(
    Path((track, module)): Path<(String, String)>,
    Query(query): Query<ModulePaginationQuery>,
) -> impl IntoResponse {
    let mut payload = json!({
        "track": track,
        "module": module
    });
    if let Some(limit) = query.limit {
        payload["limit"] = json!(limit);
    }
    if let Some(next_token) = query.next_token {
        payload["next_token"] = json!(next_token);
    }
    if let Some(include_deprecated) = query.include_deprecated {
        payload["include_deprecated"] = json!(include_deprecated);
    }
    if let Some(include_dev000) = query.include_dev000 {
        payload["include_dev000"] = json!(include_dev000);
    }
    handle_result(handlers::get_all_versions_for_module(&payload).await).await
}

async fn get_all_versions_for_stack(
    Path((track, stack)): Path<(String, String)>,
    Query(query): Query<ModulePaginationQuery>,
) -> impl IntoResponse {
    let mut payload = json!({
        "track": track,
        "stack": stack
    });
    if let Some(limit) = query.limit {
        payload["limit"] = json!(limit);
    }
    if let Some(next_token) = query.next_token {
        payload["next_token"] = json!(next_token);
    }
    if let Some(include_deprecated) = query.include_deprecated {
        payload["include_deprecated"] = json!(include_deprecated);
    }
    if let Some(include_dev000) = query.include_dev000 {
        payload["include_dev000"] = json!(include_dev000);
    }
    handle_result(handlers::get_all_versions_for_stack(&payload).await).await
}

async fn get_provider_version(
    Path((track, provider, version)): Path<(String, String, String)>,
) -> impl IntoResponse {
    handle_result(
        handlers::get_provider_version(&json!({
            "track": track,
            "provider": provider,
            "version": version
        }))
        .await,
    )
    .await
}

async fn get_provider_download_url(
    Path((track, provider, version)): Path<(String, String, String)>,
) -> impl IntoResponse {
    let result = handlers::get_provider_download_url(&json!({
        "track": track,
        "provider": provider,
        "version": version
    }))
    .await;

    match result {
        Ok(response) => response,
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({
                "error": format!("{}", e)
            })),
        )
            .into_response(),
    }
}

async fn get_job_status_http(
    Path((project, region, rest)): Path<(String, String, String)>,
) -> impl IntoResponse {
    let job_id = rest.trim_start_matches('/');
    log::info!("get_job_status_http called for job: {}", job_id);

    let payload = json!({
        "data": {
            "job_id": job_id,
            "project": project,
            "region": region
        }
    });

    let result = handlers::get_job_status(&payload).await;
    handle_result(result).await
}

// Token bridge handler - generates OIDC sign-in URL or exchanges code for tokens.
// Works with any OIDC-compliant identity provider (Cognito, Azure AD, Okta, Auth0, etc.).
// Requires OIDC_ISSUER_URL + OIDC_CLIENT_ID (or explicit endpoint env vars) to be configured.
async fn handle_auth_token(headers: HeaderMap, Json(body): Json<Value>) -> impl IntoResponse {
    use crate::auth_handler;

    // Check if this is a token exchange request (has authorization code)
    if let Some(code) = body.get("code").and_then(|v| v.as_str()) {
        // Token exchange flow: code -> tokens
        let redirect_uri = body
            .get("redirect_uri")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let code_verifier = body
            .get("code_verifier")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        match auth_handler::exchange_code_for_tokens(code, redirect_uri, code_verifier).await {
            Ok(token_response) => return (StatusCode::OK, Json(token_response)).into_response(),
            Err(e) => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({
                        "error": format!("Failed to exchange code for tokens: {}", e)
                    })),
                )
                    .into_response();
            }
        }
    }

    // Check if this is a token refresh request
    if body.get("grant_type").and_then(|v| v.as_str()) == Some("refresh_token") {
        let refresh_token = match body.get("refresh_token").and_then(|v| v.as_str()) {
            Some(rt) => rt,
            None => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({
                        "success": false,
                        "error": "Missing refresh_token field"
                    })),
                )
                    .into_response();
            }
        };
        match auth_handler::refresh_tokens(refresh_token).await {
            Ok(token_response) => return (StatusCode::OK, Json(token_response)).into_response(),
            Err(e) => {
                let status = if let Some(tre) = e.downcast_ref::<auth_handler::TokenRefreshError>()
                {
                    StatusCode::from_u16(tre.upstream_status)
                        .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR)
                } else {
                    StatusCode::INTERNAL_SERVER_ERROR
                };
                return (
                    status,
                    Json(json!({
                        "success": false,
                        "error": format!("Failed to refresh tokens: {}", e)
                    })),
                )
                    .into_response();
            }
        }
    }

    // Sign-in URL flow: generate OIDC authorization URL
    if let Some(user) = headers.get("x-auth-user").and_then(|v| v.to_str().ok()) {
        log::info!("Generating sign-in URL for authenticated user: {}", user);
    }

    let redirect_uri = body
        .get("redirect_uri")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let code_challenge = body
        .get("code_challenge")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let code_challenge_method = body
        .get("code_challenge_method")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    match auth_handler::generate_sign_in_url(redirect_uri, code_challenge, code_challenge_method)
        .await
    {
        Ok(response) => (StatusCode::OK, Json(response)).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({
                "error": format!("Failed to generate sign-in URL: {}", e)
            })),
        )
            .into_response(),
    }
}

async fn get_meta_info() -> impl IntoResponse {
    // Prefer the cloud-agnostic REGION var; fall back to AWS_REGION for backwards compatibility
    let region = std::env::var("REGION")
        .or_else(|_| std::env::var("AWS_REGION"))
        .unwrap_or_else(|_| "unknown".to_string());
    Json(json!({
        "region": region,
        "service": "infraweave-internal-api",
        "version": env!("CARGO_PKG_VERSION")
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use tower::ServiceExt;

    #[tokio::test]
    async fn change_records_route_is_not_handled_as_publish_endpoint() {
        let response = create_router()
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/api/v1/change_records/135808927253/us-west-2/cli%2Fdefault/s3bucket%2Fmy-s3bucket2?limit=5&change_type=mutate")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_ne!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
    }

    #[tokio::test]
    async fn deployment_action_missing_body_field_returns_bad_request() {
        let response = create_router()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/api/v1/apply")
                    .header("x-auth-user", "test-user")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"environment":"cli/default"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn prepared_deployment_command_mismatch_returns_bad_request() {
        let payload = serde_json::to_value(env_defs::ApiInfraPayload {
            command: "plan".to_string(),
            flags: vec![],
            module: "s3bucket".to_string(),
            module_version: "1.0.0".to_string(),
            module_type: "module".to_string(),
            module_track: "stable".to_string(),
            name: "bucket1".to_string(),
            environment: "cli/default".to_string(),
            deployment_id: "s3bucket/bucket1".to_string(),
            project_id: "123456789012".to_string(),
            region: "us-west-2".to_string(),
            drift_detection: env_defs::DriftDetection {
                enabled: false,
                interval: env_defs::DEFAULT_DRIFT_DETECTION_INTERVAL.to_string(),
                auto_remediate: false,
                webhooks: vec![],
            },
            next_drift_check_epoch: -1,
            annotations: json!({}),
            dependencies: vec![],
            initiated_by: "test-user".to_string(),
            cpu: "256".to_string(),
            memory: "512".to_string(),
            reference: "test".to_string(),
            extra_data: env_defs::ExtraData::None,
        })
        .unwrap();

        let body = json!({
            "payload": payload,
            "variables": {}
        });

        let response = create_router()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/api/v1/apply")
                    .header("x-auth-user", "test-user")
                    .header("content-type", "application/json")
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn publish_rejects_top_level_track_mismatch() {
        let body = json!({
            "zip_base64": "AA==",
            "track": "dev",
            "version": "1.0.0-dev",
            "job_id": "job-1",
            "module": {
                "module": "s3bucket",
                "module_name": "S3Bucket",
                "track": "stable",
                "version": "1.0.0"
            }
        });

        let response = create_router()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/api/v1/module/publish")
                    .header("content-type", "application/json")
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[cfg(not(feature = "local"))]
    #[tokio::test]
    async fn stack_publish_accepts_embedded_track_without_top_level_fields() {
        let body = json!({
            "zip_base64": "AA==",
            "module": {
                "module": "bucketcollection",
                "module_name": "BucketCollection",
                "track": "dev",
                "version": "1.0.0-dev"
            }
        });

        let response = create_router()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/api/v1/stack/publish")
                    .header("content-type", "application/json")
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    // The `local` feature compiles in an auth bypass (missing x-auth-user is
    // allowed), so these auth-requirement assertions only hold with it off.
    #[cfg(not(feature = "local"))]
    #[tokio::test]
    async fn provider_download_requires_auth_context() {
        let response = create_router()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/api/v1/provider/download")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"s3_key":"aws/aws-1.0.0.zip"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[cfg(not(feature = "local"))]
    #[tokio::test]
    async fn project_routes_require_auth_context_before_project_check() {
        let response = create_router()
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/api/v1/deployments/135808927253/us-west-2")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[cfg(not(feature = "local"))]
    #[tokio::test]
    async fn policy_publish_route_requires_publish_auth_context() {
        let response = create_router()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/api/v1/policy/publish")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"zip_base64":"AA==","policy":{"policy":"guardrail","environment":"default"}}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }
}
