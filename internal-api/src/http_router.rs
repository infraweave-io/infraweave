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

use env_common::errors::ModuleError;

use crate::handlers;

fn status_code_for_module_error(e: &ModuleError) -> StatusCode {
    match e {
        ModuleError::ModuleVersionExists(_, _) => StatusCode::CONFLICT,
        ModuleError::ValidationError(_) => StatusCode::CONFLICT,
        ModuleError::InvalidTrack(_)
        | ModuleError::InvalidTrackPrereleaseVersion(_, _)
        | ModuleError::InvalidStableVersion
        | ModuleError::InvalidModuleSchema(_)
        | ModuleError::InvalidExampleVariable(_)
        | ModuleError::InvalidVariableNaming(_)
        | ModuleError::InvalidOutputNaming(_)
        | ModuleError::InvalidReference(_, _)
        | ModuleError::ModuleVersionNotSet(_)
        | ModuleError::ModuleVersionMissing(_)
        | ModuleError::DuplicateClaimNames(_)
        | ModuleError::CircularDependency(_)
        | ModuleError::SelfReferencingClaim(_, _, _)
        | ModuleError::StackModuleNamespaceIsSet(_)
        | ModuleError::TerraformLockfileExists()
        | ModuleError::TerraformLockfileEmpty
        | ModuleError::TerraformNoLockfile(_)
        | ModuleError::NoProvidersDefined(_)
        | ModuleError::NoRequiredProvidersDefined(_)
        | ModuleError::OutputKeyNotFound(_, _, _, _, _)
        | ModuleError::StackClaimReferenceNotFound(_, _, _, _)
        | ModuleError::UnresolvedReference(_, _) => StatusCode::BAD_REQUEST,
        ModuleError::ModuleVersionNotFound(_, _) => StatusCode::NOT_FOUND,
        ModuleError::UploadModuleError(_)
        | ModuleError::ZipError(_)
        | ModuleError::PublishError(_)
        | ModuleError::Other(_) => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

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

            // Try to downcast to ModuleError for typed status code mapping
            let status = if let Some(module_err) = e.downcast_ref::<ModuleError>() {
                status_code_for_module_error(module_err)
            } else if err_msg.to_lowercase().contains("not found") {
                StatusCode::NOT_FOUND
            } else {
                StatusCode::INTERNAL_SERVER_ERROR
            };

            if status == StatusCode::INTERNAL_SERVER_ERROR {
                error!("Request failed: {:?}", e);
            }

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

/// Middleware that enforces publish permissions based on JWT claims.
///
/// Extracts the resource type from the URL path and the resource name from
/// either path parameters (for deprecate) or the request body (for publish).
/// Checks the `custom:publish_permissions` JWT claim for a matching pattern.
async fn publish_auth_middleware(headers: HeaderMap, request: Request, next: Next) -> Response {
    let path = request.uri().path().to_string();
    let method = request.method().clone();

    // Determine resource type and name from the request
    let (resource_type, resource_name) = if method == Method::PUT && path.contains("/deprecate") {
        // Deprecate routes:
        //   /api/v1/module/:track/:module/:version/deprecate
        //   /api/v1/stack/:track/:stack/:version/deprecate
        let segments: Vec<&str> = path.split('/').collect();
        // segments: ["", "api", "v1", "module"|"stack", track, name, version, "deprecate"]
        if segments.len() >= 6 {
            let res_type = if segments[3] == "stack" {
                "stack"
            } else {
                "module"
            };
            (res_type.to_string(), segments[5].to_string())
        } else {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": "Invalid deprecate path" })),
            )
                .into_response();
        }
    } else if method == Method::POST {
        // Publish routes: read body to extract resource name
        let (parts, body) = request.into_parts();
        let bytes = match axum::body::to_bytes(body, 512 * 1024 * 1024).await {
            Ok(b) => b,
            Err(e) => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({ "error": format!("Failed to read request body: {}", e) })),
                )
                    .into_response();
            }
        };

        let body_json: Value = match serde_json::from_slice(&bytes) {
            Ok(v) => v,
            Err(e) => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({ "error": format!("Invalid JSON body: {}", e) })),
                )
                    .into_response();
            }
        };

        let (res_type, res_name) = if path.contains("/module/publish") {
            let name = body_json
                .get("module")
                .and_then(|m| m.get("module").or_else(|| m.get("module_name")))
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .to_string();
            ("module".to_string(), name)
        } else if path.contains("/stack/publish") {
            let name = body_json
                .get("module")
                .and_then(|m| m.get("module").or_else(|| m.get("module_name")))
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .to_string();
            ("stack".to_string(), name)
        } else if path.contains("/provider/publish") {
            let name = body_json
                .get("provider")
                .and_then(|p| p.get("name"))
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .to_string();
            ("provider".to_string(), name)
        } else {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": "Unknown publish endpoint" })),
            )
                .into_response();
        };

        // Reconstruct the request with the buffered body
        let request = Request::from_parts(parts, axum::body::Body::from(bytes));
        // Check permissions before continuing
        if let Err(e) = ensure_publish_access(&headers, &res_type, &res_name).await {
            return e.into_response();
        }
        return next.run(request).await;
    } else {
        return (
            StatusCode::METHOD_NOT_ALLOWED,
            Json(json!({ "error": "Unsupported method for publish endpoint" })),
        )
            .into_response();
    };

    // Check permissions (for non-POST paths like deprecate)
    if let Err(e) = ensure_publish_access(&headers, &resource_type, &resource_name).await {
        return e.into_response();
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
        .route("/api/v1/claim/run", post(run_claim))
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

    // Routes that require publish permission (JWT custom:publish_permissions claim)
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
        .layer(middleware::from_fn(publish_auth_middleware));

    open_routes
        .merge(protected_routes)
        .merge(publish_protected_routes)
        // Add CORS layer
        .layer(cors)
    // NOTE: CompressionLayer removed because API Gateway v2 HTTP API strips the
    // Content-Encoding header, causing clients to receive compressed data without
    // knowing it's compressed. Use CloudFront for compression instead.
}

/// Extract and decode JWT token from Authorization header without signature validation.
/// We only read the claims without verifying the signature; the signature is validated
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
            None
        }
    }
}

async fn ensure_access(
    headers: &HeaderMap,
    project_id: &str,
) -> Result<(), (StatusCode, axum::response::Json<serde_json::Value>)> {
    if let Some(_user_id) = headers.get("x-auth-user").and_then(|v| v.to_str().ok()) {
        // Extract JWT claims from Authorization header
        if let Some(claims) = extract_jwt_claims(headers) {
            // Check for allowed_projects claim (configurable via AUTH_ALLOWED_PROJECTS_CLAIM)
            let claim_key = crate::auth_handler::allowed_projects_claim_key();
            if let Some(allowed_projects_str) = claims.get(&claim_key).and_then(|v| v.as_str()) {
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

        // No allowed_projects claim found in JWT; deny access.
        log::warn!(
            "User has no '{}' claim in JWT; denying access to project",
            crate::auth_handler::allowed_projects_claim_key(),
        );
        Err((
            StatusCode::FORBIDDEN,
            Json(json!({
                "error": "Access denied: no allowed_projects claim found in token"
            })),
        ))
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

/// Check if any of the user's publish permission patterns authorize the given resource.
///
/// Patterns are comma-separated in the JWT publish permissions claim
/// (configurable via `AUTH_PUBLISH_PERMISSIONS_CLAIM`, default: `custom:publish_permissions`).
///
/// Format: `{type}/{name_pattern}`
///   - `*` alone grants access to publish anything
///   - `module/{name}` grants access to publish a specific module (any track)
///   - `module/eksaddon-*` grants access to publish any module starting with `eksaddon-`
///   - `provider/{name}` grants access to publish a specific provider
///   - `provider/*` grants access to publish any provider
///
/// Examples of publish permissions claim values:
///   `module/s3bucket`                   → can only publish the s3bucket module
///   `module/s3bucket,module/eksaddon-*` → can publish s3bucket and any eksaddon-* module
///   `module/*,provider/*`               → can publish any module and any provider
///   `*`                                 → can publish anything
fn matches_publish_permission(
    permissions: &[String],
    resource_type: &str,
    resource_name: &str,
) -> bool {
    for pattern in permissions {
        let pattern = pattern.trim();

        // Global wildcard
        if pattern == "*" {
            return true;
        }

        let parts: Vec<&str> = pattern.splitn(2, '/').collect();
        if parts.len() != 2 {
            continue;
        }

        let (pattern_type, pattern_name) = (parts[0], parts[1]);

        // Type must match
        if pattern_type != resource_type {
            continue;
        }

        // Check name match with glob support
        if pattern_name == "*" {
            return true;
        } else if pattern_name.ends_with('*') {
            // Prefix glob: "eksaddon-*" matches "eksaddon-vpc", "eksaddon-iam", etc.
            let prefix = &pattern_name[..pattern_name.len() - 1];
            if resource_name.starts_with(prefix) {
                return true;
            }
        } else if pattern_name == resource_name {
            // Exact match
            return true;
        }
    }
    false
}

/// Ensure the authenticated user has permission to publish a specific resource.
///
/// Authorization is based solely on the JWT publish permissions claim
/// (a comma-separated list of patterns). If the claim is absent, access is denied.
/// The claim key is configurable via `AUTH_PUBLISH_PERMISSIONS_CLAIM` env var
/// (default: `custom:publish_permissions`).
///
/// For CI/CD pipelines: configure the pipeline's identity in your identity provider
/// with the appropriate publish permissions attribute,
/// e.g. `module/s3bucket` to restrict that pipeline to only publishing the s3bucket module.
async fn ensure_publish_access(
    headers: &HeaderMap,
    resource_type: &str,
    resource_name: &str,
) -> Result<(), (StatusCode, axum::response::Json<serde_json::Value>)> {
    let resource_desc = format!("{}/{}", resource_type, resource_name);

    if let Some(_user_id) = headers.get("x-auth-user").and_then(|v| v.to_str().ok()) {
        // Check JWT claims for publish permissions (configurable via AUTH_PUBLISH_PERMISSIONS_CLAIM)
        if let Some(claims) = extract_jwt_claims(headers) {
            let claim_key = crate::auth_handler::publish_permissions_claim_key();
            if let Some(perms_str) = claims.get(&claim_key).and_then(|v| v.as_str()) {
                let permissions: Vec<String> = perms_str
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect();

                if matches_publish_permission(&permissions, resource_type, resource_name) {
                    log::info!("User authorized to publish {} via JWT claim", resource_desc);
                    return Ok(());
                } else {
                    log::warn!("User denied publish access to {}", resource_desc,);
                    return Err((
                        StatusCode::FORBIDDEN,
                        Json(json!({
                            "error": format!(
                                "You do not have permission to publish {}. Your publish_permissions ({}) do not match this resource. Contact your administrator to update your permissions.",
                                resource_desc,
                                perms_str
                            )
                        })),
                    ));
                }
            }
        }

        // No publish_permissions claim found at all → deny
        log::warn!("User has no publish_permissions claim in JWT");
        Err((
            StatusCode::FORBIDDEN,
            Json(json!({
                "error": "You do not have permission to publish. No publish_permissions claim found. Contact your administrator to configure publish access."
            })),
        ))
    } else {
        #[cfg(feature = "local")]
        {
            log::warn!(
                "Missing x-auth-user header, allowing publish access to {} (LOCAL MODE ONLY)",
                resource_desc
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

async fn deprecate_stack(
    Path((track, stack, version)): Path<(String, String, String)>,
    Json(body): Json<DeprecateModuleBody>,
) -> impl IntoResponse {
    handle_result(
        handlers::deprecate_stack(&json!({
            "track": track,
            "stack": stack,
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

/// Stack publish handler - stacks use the same ModuleResp format as modules,
/// but the server-side handler also ensures provider caches exist in all regions.
async fn publish_stack(Json(body): Json<PublishModuleBody>) -> impl IntoResponse {
    handle_result(
        handlers::publish_stack(&json!({
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

#[derive(Deserialize)]
struct PublishProviderBody {
    zip_base64: String,
    provider: Value,
}

async fn publish_provider(Json(body): Json<PublishProviderBody>) -> impl IntoResponse {
    handle_result(
        handlers::publish_provider(&json!({
            "zip_base64": body.zip_base64,
            "provider": body.provider
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
