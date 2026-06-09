use axum::{
    extract::Json,
    http::HeaderMap,
    response::{IntoResponse, Response},
};
use serde_json::{json, Value};

use crate::{handlers, http_authz::ensure_access, http_response::handle_result};

pub(crate) async fn run_claim(headers: HeaderMap, Json(body): Json<Value>) -> Response {
    log::info!("Received run_claim request");

    let payload_value = match body.get("payload") {
        Some(p) => p.clone(),
        None => {
            return handle_result(Err(anyhow::anyhow!("Missing 'payload' field")))
                .await
                .into_response()
        }
    };

    let variables = match body.get("variables") {
        Some(v) => v.clone(),
        None => {
            return handle_result(Err(anyhow::anyhow!("Missing 'variables' field")))
                .await
                .into_response()
        }
    };

    let payload: env_defs::ApiInfraPayload = match serde_json::from_value(payload_value.clone()) {
        Ok(p) => p,
        Err(e) => {
            return handle_result(Err(anyhow::anyhow!("Invalid payload: {}", e)))
                .await
                .into_response()
        }
    };

    if let Err(e) = ensure_access(&headers, &payload.project_id).await {
        return e.into_response();
    }

    handlers::with_workload_account(
        payload.project_id.clone(),
        run_claim_authorized(payload_value, variables, payload),
    )
    .await
}

async fn run_claim_authorized(
    payload_value: Value,
    variables: Value,
    payload: env_defs::ApiInfraPayload,
) -> Response {
    let result = handlers::start_runner(&json!({
        "data": payload_value
    }))
    .await;

    let task_arn = match result {
        Ok(resp) => resp["task_arn"].as_str().unwrap_or("").to_string(),
        Err(e) => return handle_result(Err(e)).await.into_response(),
    };

    let task_id = task_arn.split('/').last().unwrap_or(&task_arn).to_string();

    log::info!("Task ARN: {}, Task ID: {}", task_arn, task_id);

    if let Err(e) = insert_deployment_record(&payload, &variables, &task_id).await {
        log::error!("Failed to insert deployment record: {}", e);
        return handle_result(Err(e)).await.into_response();
    }

    handle_result(Ok(json!({
        "task_arn": task_arn,
        "job_id": task_id
    })))
    .await
    .into_response()
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
