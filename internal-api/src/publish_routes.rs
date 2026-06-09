use axum::{
    extract::{Json, Path},
    response::IntoResponse,
};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::{handlers, http_response::handle_result};

#[derive(Deserialize)]
pub(crate) struct DeprecateModuleBody {
    message: Option<String>,
}

pub(crate) async fn deprecate_module(
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

pub(crate) async fn deprecate_stack(
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
pub(crate) struct PublishModuleBody {
    zip_base64: String,
    module: Value,
    track: String,
    version: String,
    job_id: String,
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

pub(crate) async fn publish_module(Json(body): Json<PublishModuleBody>) -> impl IntoResponse {
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

pub(crate) async fn publish_stack(Json(body): Json<PublishModuleBody>) -> impl IntoResponse {
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
pub(crate) struct PublishProviderBody {
    zip_base64: String,
    provider: Value,
}

#[derive(Deserialize)]
pub(crate) struct PublishPolicyBody {
    zip_base64: String,
    policy: Value,
}

pub(crate) async fn publish_provider(Json(body): Json<PublishProviderBody>) -> impl IntoResponse {
    handle_result(
        handlers::publish_provider(&json!({
            "zip_base64": body.zip_base64,
            "provider": body.provider
        }))
        .await,
    )
    .await
}

pub(crate) async fn publish_policy(Json(body): Json<PublishPolicyBody>) -> impl IntoResponse {
    handle_result(
        handlers::publish_policy(&json!({
            "zip_base64": body.zip_base64,
            "policy": body.policy
        }))
        .await,
    )
    .await
}
