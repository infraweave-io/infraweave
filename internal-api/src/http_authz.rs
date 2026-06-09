use axum::{
    extract::{Path, Request},
    http::{HeaderMap, Method, StatusCode},
    middleware::Next,
    response::{IntoResponse, Json, Response},
};
use serde_json::{json, Value};
use std::collections::HashMap;

pub(crate) async fn auth_middleware(
    headers: HeaderMap,
    Path(params): Path<HashMap<String, String>>,
    request: Request,
    next: Next,
) -> Response {
    if let Err(e) = ensure_authenticated_user(&headers) {
        return e.into_response();
    }

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

pub(crate) async fn publish_auth_middleware(
    headers: HeaderMap,
    request: Request,
    next: Next,
) -> Response {
    let path = request.uri().path().to_string();
    let method = request.method().clone();

    let (resource_type, resource_name, resource_track) =
        if method == Method::PUT && path.contains("/deprecate") {
            let segments: Vec<&str> = path.split('/').collect();
            if segments.len() >= 6 {
                let res_type = if segments[3] == "stack" {
                    "stack"
                } else {
                    "module"
                };
                (
                    res_type.to_string(),
                    segments[5].to_string(),
                    Some(segments[4].to_string()),
                )
            } else {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({ "error": "Invalid deprecate path" })),
                )
                    .into_response();
            }
        } else if method == Method::POST {
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
            } else if path.contains("/policy/publish") {
                let name = body_json
                    .get("policy")
                    .and_then(|p| p.get("policy"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown")
                    .to_string();
                ("policy".to_string(), name)
            } else {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({ "error": "Unknown publish endpoint" })),
                )
                    .into_response();
            };
            let track = body_json
                .get("track")
                .and_then(|v| v.as_str())
                .map(str::to_string)
                .or_else(|| {
                    body_json
                        .get("module")
                        .and_then(|m| m.get("track"))
                        .and_then(|v| v.as_str())
                        .map(str::to_string)
                });

            let request = Request::from_parts(parts, axum::body::Body::from(bytes));
            if let Err(e) =
                ensure_publish_access(&headers, &res_type, &res_name, track.as_deref()).await
            {
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

    if let Err(e) = ensure_publish_access(
        &headers,
        &resource_type,
        &resource_name,
        resource_track.as_deref(),
    )
    .await
    {
        return e.into_response();
    }
    next.run(request).await
}

pub(crate) fn extract_jwt_claims(headers: &HeaderMap) -> Option<Value> {
    if let Some(claims) = extract_verified_claims(headers) {
        return Some(claims);
    }

    let auth_header = headers.get("authorization").and_then(|v| v.to_str().ok())?;
    let token = auth_header
        .strip_prefix("Bearer ")
        .or_else(|| auth_header.strip_prefix("bearer "))?;

    let parts: Vec<&str> = token.split('.').collect();
    if parts.len() != 3 {
        log::warn!("Invalid JWT format: expected 3 parts, got {}", parts.len());
        return None;
    }

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

fn extract_verified_claims(headers: &HeaderMap) -> Option<Value> {
    let claims_header = headers
        .get("x-infraweave-verified-claims")
        .and_then(|v| v.to_str().ok())?;

    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};

    let payload_bytes = match URL_SAFE_NO_PAD.decode(claims_header) {
        Ok(bytes) => bytes,
        Err(e) => {
            log::warn!(
                "Failed to decode x-infraweave-verified-claims header: {}",
                e
            );
            return None;
        }
    };

    match serde_json::from_slice(&payload_bytes) {
        Ok(claims) => Some(claims),
        Err(e) => {
            log::error!(
                "Failed to parse x-infraweave-verified-claims as JSON: {}",
                e
            );
            None
        }
    }
}

pub(crate) async fn ensure_access(
    headers: &HeaderMap,
    project_id: &str,
) -> Result<(), (StatusCode, Json<serde_json::Value>)> {
    if let Some(_user_id) = headers.get("x-auth-user").and_then(|v| v.to_str().ok()) {
        if let Some(claims) = extract_jwt_claims(headers) {
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

fn ensure_authenticated_user(
    headers: &HeaderMap,
) -> Result<(), (StatusCode, Json<serde_json::Value>)> {
    if headers
        .get("x-auth-user")
        .and_then(|v| v.to_str().ok())
        .is_some()
    {
        return Ok(());
    }

    #[cfg(feature = "local")]
    {
        log::warn!(
            "Missing x-auth-user header, allowing authenticated-only route (LOCAL MODE ONLY)"
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

async fn ensure_publish_access(
    headers: &HeaderMap,
    resource_type: &str,
    resource_name: &str,
    track: Option<&str>,
) -> Result<(), (StatusCode, Json<serde_json::Value>)> {
    let resource_desc = match track {
        Some(track) => format!("{}/{}/{}", resource_type, resource_name, track),
        None => format!("{}/{}", resource_type, resource_name),
    };

    if let Some(claims) = extract_verified_claims(headers) {
        if crate::publish_auth::check_publish(&claims, resource_type, resource_name, track).await {
            log::info!(
                "User authorized to publish {} via Rego policy",
                resource_desc
            );
            return Ok(());
        }

        log::warn!("User has no matching publish grant for {}", resource_desc);
        Err((
            StatusCode::FORBIDDEN,
            Json(json!({
                "error": format!("You do not have permission to publish {}. Configure AUTH_PUBLISH_REGO_POLICY_PARAMETER with the Rego publish policy.", resource_desc)
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
