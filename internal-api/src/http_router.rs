use axum::{
    extract::{Path, Query, Request},
    http::{HeaderMap, Method, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Json, Response},
    routing::{get, post, put},
    Router,
};
use log::error;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use tower_http::cors::{Any, CorsLayer};

use crate::handlers;

// Helper function to handle responses consistently
async fn handle_result(result: anyhow::Result<Value>) -> impl IntoResponse {
    match result {
        Ok(mut response) => {
            // Check if response is an object with "Items"
            if let Some(obj) = response.as_object_mut() {
                if obj.contains_key("Items") {
                    // It's a list response
                    let items = obj.remove("Items").unwrap();

                    // Check for pagination token
                    let mut headers = axum::http::HeaderMap::new();

                    // Handle "next_token" if it exists
                    if let Some(next_token) = obj.remove("next_token") {
                        if let Some(token_str) = next_token.as_str() {
                            if let Ok(val) = axum::http::HeaderValue::from_str(token_str) {
                                headers.insert("x-next-token", val);
                            }
                        }
                    }

                    return (StatusCode::OK, headers, Json(items)).into_response();
                }
            }

            // Default behavior for non-list responses
            (StatusCode::OK, Json(response)).into_response()
        }
        Err(e) => {
            let err_msg = e.to_string();
            let status = if err_msg.to_lowercase().contains("not found") {
                StatusCode::NOT_FOUND
            } else {
                error!("Request failed: {:?}", e);
                StatusCode::INTERNAL_SERVER_ERROR
            };

            // Include full error details (including cause chain) for internal server errors
            // This aids debugging client-side when using the API directly
            let response_msg = if status == StatusCode::INTERNAL_SERVER_ERROR {
                format!("{:?}", e)
            } else {
                err_msg
            };

            (
                status,
                Json(json!({
                    "error": response_msg
                })),
            )
                .into_response()
        }
    }
}

async fn auth_middleware(
    headers: HeaderMap,
    Path(params): Path<HashMap<String, String>>,
    request: Request,
    next: Next,
) -> Response {
    if let Some(project_param) = params.get("project") {
        for project in project_param.split(',') {
            let p = project.trim();
            if !p.is_empty() {
                if let Err(e) = ensure_access(&headers, p).await {
                    return e.into_response();
                }
            }
        }
    }
    next.run(request).await
}

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
            "/api/v1/deployment/:project/:region/*rest",
            get(describe_deployment),
        )
        // Multi-project deployments list supported via comma-separated :project param
        .route("/api/v1/deployments/:project/:region", get(get_deployments))
        .route(
            "/api/v1/deployments/module/:project/:region/:module",
            get(get_deployments_for_module),
        )
        .route(
            "/api/v1/deployments/history/:project/:region",
            get(get_deployments_history),
        )
        // Specific endpoint for deployment plan status by job_id
        .route(
            "/api/v1/plan/:project/:region/*rest",
            get(describe_plan_deployment),
        )
        .route("/api/v1/logs/:project/:region/:job_id", get(read_logs))
        .route("/api/v1/events/:project/:region/*rest", get(get_events))
        .route(
            "/api/v1/change_record/:project/:region/*rest",
            get(get_change_record),
        )
        .route(
            "/api/v1/change_record_graph/:project/:region/*rest",
            get(get_change_record_graph),
        )
        .route(
            "/api/v1/deployment_graph/:project/:region/*rest",
            get(get_deployment_graph),
        )
        // Provider download route - returns base64 content (requires auth)
        .route("/api/v1/provider/download", post(download_provider))
        // Plan/Apply/Destroy operations
        .route("/api/v1/claim/run", post(run_claim))
        // Job status route - use wildcard to handle ARNs with slashes
        .route(
            "/api/v1/job_status/:project/:region/*rest",
            get(get_job_status_http),
        )
        .layer(middleware::from_fn(auth_middleware));

    // Open routes / Global lookups
    let open_routes = Router::new()
        .route(
            "/2015-03-31/functions/:function_name/invocations",
            post(handlers::handle_lambda_invocation),
        )
        // Authentication / Token bridge route
        .route("/api/v1/auth/token", post(get_token_for_iam_user))
        // Meta endpoint for region discovery
        // MUST be unauthenticated to allow clients to discover region via Latency Based Routing
        // before they can sign requests with the correct region.
        .route("/api/v1/meta", get(get_meta_info))
        .route("/api/v1/modules", get(get_modules))
        .route("/api/v1/projects", get(get_projects))
        .route("/api/v1/stacks", get(get_stacks))
        .route("/api/v1/providers", get(get_providers))
        .route(
            "/api/v1/module/:track/:module_name/:module_version",
            get(get_module_version),
        )
        .route(
            "/api/v1/module/:track/:module_name/:module_version/download",
            get(get_module_download_url),
        )
        .route(
            "/api/v1/stack/:track/:stack_name/:stack_version",
            get(get_stack_version),
        )
        .route(
            "/api/v1/stack/:track/:stack_name/:stack_version/download",
            get(get_stack_download_url),
        )
        .route(
            "/api/v1/modules/versions/:track/:module",
            get(get_all_versions_for_module),
        )
        .route(
            "/api/v1/stacks/versions/:track/:stack",
            get(get_all_versions_for_stack),
        )
        .route(
            "/api/v1/provider/:track/:provider/:version",
            get(get_provider_version),
        )
        .route(
            "/api/v1/provider/:track/:provider/:version/download",
            get(get_provider_download_url),
        )
        // Policy routes
        .route("/api/v1/policies/:environment", get(get_policies))
        .route(
            "/api/v1/policy/:environment/:policy_name/:policy_version",
            get(get_policy_version),
        )
        // Module deprecation route
        .route(
            "/api/v1/module/:track/:module/:version/deprecate",
            put(deprecate_module),
        )
        // Module publish route - accepts pre-built modules
        .route("/api/v1/module/publish", post(publish_module))
        // Job status route
        .route(
            "/api/v1/module/publish/:job_id",
            get(get_publish_job_status),
        );

    open_routes
        .merge(protected_routes)
        // Add CORS layer
        .layer(cors)
    // NOTE: CompressionLayer removed because API Gateway v2 HTTP API strips the
    // Content-Encoding header, causing clients to receive compressed data without
    // knowing it's compressed. Use CloudFront for compression instead.
}

/// Extract and decode JWT token from Authorization header without signature validation.
/// We only read the claims without verifying the signature — the signature is validated
/// upstream by the platform (API Gateway on AWS, or Azure App Service EasyAuth on Azure)
/// before the request ever reaches this service.
fn extract_jwt_claims(headers: &HeaderMap) -> Option<Value> {
    let auth_header = headers.get("authorization").and_then(|v| v.to_str().ok())?;

    // Remove "Bearer " prefix
    let token = auth_header
        .strip_prefix("Bearer ")
        .or_else(|| auth_header.strip_prefix("bearer "))?;

    // JWT format: header.payload.signature
    let parts: Vec<&str> = token.split('.').collect();
    if parts.len() != 3 {
        log::warn!("Invalid JWT format: expected 3 parts, got {}", parts.len());
        return None;
    }

    // Decode the payload (second part) without verification
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
    let payload_bytes = match URL_SAFE_NO_PAD.decode(parts[1]) {
        Ok(bytes) => bytes,
        Err(e) => {
            log::warn!("Failed to decode JWT payload: {}", e);
            return None;
        }
    };

    match serde_json::from_slice(&payload_bytes) {
        Ok(claims) => Some(claims),
        Err(e) => {
            log::error!("Failed to parse JWT claims as JSON: {}", e);
            log::debug!(
                "Raw payload bytes: {:?}",
                String::from_utf8_lossy(&payload_bytes)
            );
            None
        }
    }
}

async fn ensure_access(
    headers: &HeaderMap,
    project_id: &str,
) -> Result<(), (StatusCode, axum::response::Json<serde_json::Value>)> {
    if let Some(user_id) = headers.get("x-auth-user").and_then(|v| v.to_str().ok()) {
        // Extract JWT claims from Authorization header
        if let Some(claims) = extract_jwt_claims(headers) {
            // Check for custom:allowed_projects in JWT claims
            if let Some(allowed_projects_str) = claims
                .get("custom:allowed_projects")
                .and_then(|v| v.as_str())
            {
                let allowed_projects: Vec<String> = allowed_projects_str
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect();

                if allowed_projects.contains(&project_id.to_string()) {
                    return Ok(());
                } else {
                    return Err((
                        StatusCode::FORBIDDEN,
                        Json(json!({
                            "error": "Access denied to this project"
                        })),
                    ));
                }
            }
        }

        // Fallback to database check (legacy method)
        match handlers::check_project_access(user_id, project_id).await {
            Ok(true) => Ok(()),
            Ok(false) => Err((
                StatusCode::FORBIDDEN,
                Json(json!({
                    "error": "Access denied to this project"
                })),
            )),
            Err(e) => Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "error": format!("Authorization check failed: {}", e)
                })),
            )),
        }
    } else {
        #[cfg(feature = "local")]
        {
            log::warn!(
                "Missing x-auth-user header, allowing access to project {} (LOCAL MODE ONLY)",
                project_id
            );
            Ok(())
        }
        #[cfg(not(feature = "local"))]
        {
            Err((
                StatusCode::UNAUTHORIZED,
                Json(json!({
                    "error": "Missing authentication user context"
                })),
            ))
        }
    }
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

    // Extract allowed_projects from JWT claims
    if let Some(claims) = extract_jwt_claims(&headers) {
        if let Some(allowed_projects_str) = claims
            .get("custom:allowed_projects")
            .and_then(|v| v.as_str())
        {
            let allowed_projects: Vec<String> = allowed_projects_str
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();

            if !allowed_projects.is_empty() {
                log::info!(
                    "Using allowed_projects from JWT claims for user {}: {:?}",
                    user_id,
                    allowed_projects
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

#[derive(Deserialize)]
struct DeprecateModuleBody {
    message: Option<String>,
}

async fn deprecate_module(
    Path((track, module, version)): Path<(String, String, String)>,
    Json(body): Json<DeprecateModuleBody>,
) -> impl IntoResponse {
    handle_result(
        handlers::deprecate_module(&json!({
            "track": track,
            "module": module,
            "version": version,
            "message": body.message
        }))
        .await,
    )
    .await
}

#[derive(Deserialize)]
struct PublishModuleBody {
    zip_base64: String,
    module: Value, // ModuleResp serialized as JSON
    track: String,
    version: String,
    job_id: String,
}

async fn download_provider(Json(body): Json<Value>) -> impl IntoResponse {
    handle_result(
        handlers::download_provider(&json!({
            "s3_key": body.get("s3_key")
        }))
        .await,
    )
    .await
}

async fn publish_module(Json(body): Json<PublishModuleBody>) -> impl IntoResponse {
    handle_result(
        handlers::publish_module(&json!({
            "zip_base64": body.zip_base64,
            "module": body.module,
            "track": body.track,
            "version": body.version,
            "job_id": body.job_id
        }))
        .await,
    )
    .await
}

async fn get_publish_job_status(Path(job_id): Path<String>) -> impl IntoResponse {
    handle_result(
        handlers::get_publish_job_status(&json!({
            "job_id": job_id
        }))
        .await,
    )
    .await
}

async fn run_claim(Json(body): Json<Value>) -> impl IntoResponse {
    log::info!("Received run_claim request");
    // Body is ApiInfraPayloadWithVariables
    // Manually extract payload and variables from the JSON value
    let payload_value = match body.get("payload") {
        Some(p) => p.clone(),
        None => return handle_result(Err(anyhow::anyhow!("Missing 'payload' field"))).await,
    };

    let variables = match body.get("variables") {
        Some(v) => v.clone(),
        None => return handle_result(Err(anyhow::anyhow!("Missing 'variables' field"))).await,
    };

    let payload: env_defs::ApiInfraPayload = match serde_json::from_value(payload_value.clone()) {
        Ok(p) => p,
        Err(e) => return handle_result(Err(anyhow::anyhow!("Invalid payload: {}", e))).await,
    };

    // Launch runner with ApiInfraPayload only (no variables to avoid size limits)
    let result = handlers::start_runner(&json!({
        "data": payload_value
    }))
    .await;

    let task_arn = match result {
        Ok(resp) => resp["task_arn"].as_str().unwrap_or("").to_string(),
        Err(e) => return handle_result(Err(e)).await,
    };

    // Extract task ID from ARN: arn:aws:ecs:region:account:task/cluster/TASK_ID
    let task_id = task_arn.split('/').last().unwrap_or(&task_arn).to_string();

    log::info!("Task ARN: {}, Task ID: {}", task_arn, task_id);

    // Insert deployment record with variables into database using task ID
    // This allows the runner to query the deployment and get variables
    if let Err(e) = insert_deployment_record(&payload, &variables, &task_id).await {
        log::error!("Failed to insert deployment record: {}", e);
        return handle_result(Err(e)).await;
    }

    handle_result(Ok(json!({
        "task_arn": task_arn,
        "job_id": task_id
    })))
    .await
}

async fn insert_deployment_record(
    payload: &env_defs::ApiInfraPayload,
    variables: &serde_json::Value,
    job_id: &str,
) -> Result<(), anyhow::Error> {
    use env_common::interface::GenericCloudHandler;

    let handler = GenericCloudHandler::workload(&payload.project_id, &payload.region).await;

    let payload_with_variables = env_defs::ApiInfraPayloadWithVariables {
        payload: payload.clone(),
        variables: variables.clone(),
    };

    env_common::insert_request_event(&handler, &payload_with_variables, job_id).await
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

// Token bridge handler - generates sign-in URL or exchanges code for tokens
#[cfg(feature = "aws")]
async fn get_token_for_iam_user(headers: HeaderMap, Json(body): Json<Value>) -> impl IntoResponse {
    use crate::auth_handler;

    // Check if this is a token exchange request (has authorization code)
    if let Some(code) = body.get("code").and_then(|v| v.as_str()) {
        // Token exchange flow: code -> tokens
        let redirect_uri = body
            .get("redirect_uri")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        match auth_handler::exchange_code_for_tokens(code, redirect_uri).await {
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

    // Sign-in URL flow: verify user exists and return sign-in URL
    // Extract IAM identity from the request headers
    // This is injected by the unified handler from API Gateway context
    let user_id = if let Some(user) = headers.get("x-auth-user").and_then(|v| v.to_str().ok()) {
        user.to_string()
    } else if let Some(iam_context) = body.get("requestContext") {
        // Fallback: try to extract from request context if passed in body
        match auth_handler::extract_iam_identity(iam_context) {
            Ok(identity) => identity,
            Err(e) => {
                return (
                    StatusCode::UNAUTHORIZED,
                    Json(json!({
                        "error": format!("Failed to extract IAM identity: {}", e)
                    })),
                )
                    .into_response();
            }
        }
    } else {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({
                "error": "Missing IAM authentication. This endpoint requires IAM authorization."
            })),
        )
            .into_response();
    };

    // Get optional redirect_uri from request body
    let redirect_uri = body
        .get("redirect_uri")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    match auth_handler::generate_token_for_iam_user(&user_id, redirect_uri).await {
        Ok(token_response) => (StatusCode::OK, Json(token_response)).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({
                "error": format!("Failed to generate token: {}", e)
            })),
        )
            .into_response(),
    }
}

#[cfg(not(feature = "aws"))]
async fn get_token_for_iam_user(Json(_body): Json<Value>) -> impl IntoResponse {
    (
        StatusCode::NOT_IMPLEMENTED,
        Json(json!({
            "error": "Token bridge is only available for AWS deployments"
        })),
    )
        .into_response()
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
