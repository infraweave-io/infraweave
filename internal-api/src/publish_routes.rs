use axum::{
    extract::{Json, Path},
    http::HeaderMap,
    response::{IntoResponse, Response},
};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::{
    handlers,
    http_authz::ensure_publish_access,
    http_response::{bad_request, handle_result},
};

#[derive(Deserialize)]
pub(crate) struct DeprecateModuleBody {
    message: Option<String>,
}

pub(crate) async fn deprecate_module(
    headers: HeaderMap,
    Path((track, module, version)): Path<(String, String, String)>,
    Json(body): Json<DeprecateModuleBody>,
) -> Response {
    if let Err(e) = ensure_publish_access(&headers, "module", &module, Some(&track)).await {
        return e.into_response();
    }

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
    .into_response()
}

pub(crate) async fn deprecate_stack(
    headers: HeaderMap,
    Path((track, stack, version)): Path<(String, String, String)>,
    Json(body): Json<DeprecateModuleBody>,
) -> Response {
    if let Err(e) = ensure_publish_access(&headers, "stack", &stack, Some(&track)).await {
        return e.into_response();
    }

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
    .into_response()
}

#[derive(Deserialize)]
pub(crate) struct PublishModuleBody {
    zip_base64: String,
    module: Value,
    track: Option<String>,
    version: Option<String>,
    job_id: Option<String>,
}

fn required_string_field<'a>(
    value: &'a Value,
    field: &str,
    resource_type: &str,
) -> Result<&'a str, anyhow::Error> {
    value
        .get(field)
        .and_then(|value| value.as_str())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| bad_request(format!("Missing '{}.{}' parameter", resource_type, field)))
}

fn publish_module_resource(
    body: &PublishModuleBody,
    resource_type: &str,
) -> Result<(String, String, String), anyhow::Error> {
    let name = required_string_field(&body.module, "module", resource_type)?.to_string();
    let track = required_string_field(&body.module, "track", resource_type)?.to_string();
    let version = required_string_field(&body.module, "version", resource_type)?.to_string();

    if let Some(body_track) = &body.track {
        if body_track != &track {
            return Err(bad_request(format!(
                "Top-level track '{}' does not match {} track '{}'",
                body_track, resource_type, track
            )));
        }
    }

    if let Some(body_version) = &body.version {
        if body_version != &version {
            return Err(bad_request(format!(
                "Top-level version '{}' does not match {} version '{}'",
                body_version, resource_type, version
            )));
        }
    }

    Ok((name, track, version))
}

pub(crate) async fn download_provider(Json(body): Json<Value>) -> impl IntoResponse {
    handle_result(
        handlers::download_provider(&json!({
            "s3_key": body.get("s3_key")
        }))
        .await,
    )
    .await
    .into_response()
}

pub(crate) async fn publish_module(
    headers: HeaderMap,
    Json(body): Json<PublishModuleBody>,
) -> Response {
    let (module_name, module_track, module_version) = match publish_module_resource(&body, "module")
    {
        Ok(resource) => resource,
        Err(e) => return handle_result(Err(e)).await.into_response(),
    };

    if let Err(e) =
        ensure_publish_access(&headers, "module", &module_name, Some(&module_track)).await
    {
        return e.into_response();
    }

    let job_id = body.job_id.unwrap_or_default();

    handle_result(
        handlers::publish_module(&json!({
            "zip_base64": body.zip_base64,
            "module": body.module,
            "track": module_track,
            "version": module_version,
            "job_id": job_id
        }))
        .await,
    )
    .await
    .into_response()
}

pub(crate) async fn publish_stack(
    headers: HeaderMap,
    Json(body): Json<PublishModuleBody>,
) -> Response {
    let (stack_name, stack_track, stack_version) = match publish_module_resource(&body, "stack") {
        Ok(resource) => resource,
        Err(e) => return handle_result(Err(e)).await.into_response(),
    };

    if let Err(e) = ensure_publish_access(&headers, "stack", &stack_name, Some(&stack_track)).await
    {
        return e.into_response();
    }

    let job_id = body.job_id.unwrap_or_default();

    handle_result(
        handlers::publish_stack(&json!({
            "zip_base64": body.zip_base64,
            "module": body.module,
            "track": stack_track,
            "version": stack_version,
            "job_id": job_id
        }))
        .await,
    )
    .await
    .into_response()
}

#[derive(Deserialize)]
pub(crate) struct PublishProviderBody {
    zip_base64: String,
    provider: Value,
}

#[derive(Deserialize)]
pub(crate) struct PublishPolicyBody {
    zip_base64: String,
    policy: Value,
}

pub(crate) async fn publish_provider(
    headers: HeaderMap,
    Json(body): Json<PublishProviderBody>,
) -> Response {
    let provider_name = body
        .provider
        .get("name")
        .and_then(|value| value.as_str())
        .unwrap_or("unknown")
        .to_string();

    if let Err(e) = ensure_publish_access(&headers, "provider", &provider_name, None).await {
        return e.into_response();
    }

    handle_result(
        handlers::publish_provider(&json!({
            "zip_base64": body.zip_base64,
            "provider": body.provider
        }))
        .await,
    )
    .await
    .into_response()
}

pub(crate) async fn publish_policy(
    headers: HeaderMap,
    Json(body): Json<PublishPolicyBody>,
) -> Response {
    let policy_name = body
        .policy
        .get("policy")
        .and_then(|value| value.as_str())
        .unwrap_or("unknown")
        .to_string();

    if let Err(e) = ensure_publish_access(&headers, "policy", &policy_name, None).await {
        return e.into_response();
    }

    handle_result(
        handlers::publish_policy(&json!({
            "zip_base64": body.zip_base64,
            "policy": body.policy
        }))
        .await,
    )
    .await
    .into_response()
}
