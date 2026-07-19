use axum::{
    extract::Json,
    http::HeaderMap,
    response::{IntoResponse, Response},
};
use serde_json::{json, Value};

use crate::{
    handlers,
    http_authz::ensure_access,
    http_response::{bad_request, conflict, handle_result},
};

fn body_string_param(body: &Value, names: &[&str]) -> Result<String, anyhow::Error> {
    names
        .iter()
        .find_map(|name| body.get(*name).and_then(|value| value.as_str()))
        .map(str::to_string)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| bad_request(format!("Missing '{}' parameter", names.join("' or '"))))
}

fn optional_body_string_param(body: &Value, names: &[&str]) -> Option<String> {
    names
        .iter()
        .find_map(|name| body.get(*name).and_then(|value| value.as_str()))
        .map(str::to_string)
        .filter(|value| !value.is_empty())
}

fn body_string_array_param(body: &Value, name: &str) -> Result<Vec<String>, anyhow::Error> {
    match body.get(name) {
        Some(Value::Array(values)) => values
            .iter()
            .map(|value| {
                value
                    .as_str()
                    .map(str::to_string)
                    .ok_or_else(|| bad_request(format!("'{}' must contain only strings", name)))
            })
            .collect(),
        Some(_) => Err(bad_request(format!(
            "'{}' must be an array of strings",
            name
        ))),
        None => Ok(vec![]),
    }
}

fn body_claim_param(body: &Value) -> Result<serde_yaml::Value, anyhow::Error> {
    let claim = body
        .get("claim")
        .or_else(|| body.get("manifest"))
        .or_else(|| body.get("deployment"))
        .ok_or_else(|| bad_request("Missing 'claim' or 'manifest' parameter"))?;

    if let Some(claim_yaml) = claim.as_str() {
        serde_yaml::from_str(claim_yaml)
            .map_err(|e| bad_request(format!("Invalid claim YAML: {}", e)))
    } else {
        serde_yaml::to_value(claim)
            .map_err(|e| bad_request(format!("Invalid claim manifest: {}", e)))
    }
}

fn region_from_claim_or_body(
    body: &Value,
    claim: &serde_yaml::Value,
) -> Result<String, anyhow::Error> {
    if let Some(region) = optional_body_string_param(body, &["region"]) {
        return Ok(region);
    }

    claim
        .get("spec")
        .and_then(|spec| spec.get("region"))
        .and_then(|region| region.as_str())
        .map(str::to_string)
        .ok_or_else(|| bad_request("Missing 'region' parameter"))
}

fn deployment_selector_from_body(
    body: &Value,
) -> Result<(String, String, String, String), anyhow::Error> {
    Ok((
        body_string_param(body, &["project", "project_id"])?,
        body_string_param(body, &["region"])?,
        body_string_param(body, &["environment", "environment_id"])?,
        body_string_param(body, &["deployment_id"])?,
    ))
}

#[cfg_attr(feature = "swagger", utoipa::path(
    post,
    path = "/api/v1/apply",
    tag = "deployments",
    request_body = serde_json::Value,
    responses((status = 200, description = "Apply started (job info)"))
))]
pub(crate) async fn apply_deployment_from_body(
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    run_claim_action_from_body(headers, body, "apply").await
}

#[cfg_attr(feature = "swagger", utoipa::path(
    post,
    path = "/api/v1/plan",
    tag = "deployments",
    request_body = serde_json::Value,
    responses((status = 200, description = "Plan started (job info)"))
))]
pub(crate) async fn plan_deployment_from_body(
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    run_claim_action_from_body(headers, body, "plan").await
}

async fn run_claim_action_from_body(headers: HeaderMap, body: Value, command: &str) -> Response {
    use env_common::interface::GenericCloudHandler;
    use env_common::logic::validate_and_prepare_claim;
    use env_defs::ExtraData;

    if body.get("payload").is_some() || body.get("variables").is_some() {
        return run_prepared_deployment_action(headers, body, command).await;
    }

    let project = match body_string_param(&body, &["project", "project_id"]) {
        Ok(project) => project,
        Err(e) => return handle_result(Err(e)).await.into_response(),
    };
    let environment = match body_string_param(&body, &["environment", "environment_id"]) {
        Ok(environment) => environment,
        Err(e) => return handle_result(Err(e)).await.into_response(),
    };
    let claim = match body_claim_param(&body) {
        Ok(claim) => claim,
        Err(e) => return handle_result(Err(e)).await.into_response(),
    };
    let region = match region_from_claim_or_body(&body, &claim) {
        Ok(region) => region,
        Err(e) => return handle_result(Err(e)).await.into_response(),
    };
    let flags = match body_string_array_param(&body, "flags") {
        Ok(flags) => flags,
        Err(e) => return handle_result(Err(e)).await.into_response(),
    };

    if let Err(e) = ensure_access(&headers, &project).await {
        return e.into_response();
    }

    let handler = GenericCloudHandler::workload(&project, &region).await;
    let reference_fallback = optional_body_string_param(&body, &["reference"])
        .unwrap_or_else(|| format!("api-{}", command));

    let (_, payload_with_variables) = match validate_and_prepare_claim(
        &handler,
        &claim,
        &environment,
        command,
        flags,
        ExtraData::None,
        &reference_fallback,
    )
    .await
    {
        Ok(prepared) => prepared,
        Err(e) => {
            return handle_result(Err(bad_request(e.to_string())))
                .await
                .into_response()
        }
    };

    let payload_value = match serde_json::to_value(&payload_with_variables.payload) {
        Ok(value) => value,
        Err(e) => {
            return handle_result(Err(anyhow::anyhow!(
                "Failed to serialize {} payload: {}",
                command,
                e
            )))
            .await
            .into_response()
        }
    };

    handlers::with_workload_account(
        project,
        run_claim_authorized(
            payload_value,
            payload_with_variables.variables,
            payload_with_variables.payload,
        ),
    )
    .await
}

#[cfg_attr(feature = "swagger", utoipa::path(
    post,
    path = "/api/v1/reapply",
    tag = "deployments",
    request_body = serde_json::Value,
    responses((status = 200, description = "Reapply started (job info)"))
))]
pub(crate) async fn reapply_deployment_from_body(
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    let (project, region, environment, deployment_id) = match deployment_selector_from_body(&body) {
        Ok(selector) => selector,
        Err(e) => return handle_result(Err(e)).await.into_response(),
    };

    reapply_deployment(headers, project, region, environment, deployment_id).await
}

#[cfg_attr(feature = "swagger", utoipa::path(
    post,
    path = "/api/v1/destroy",
    tag = "deployments",
    request_body = serde_json::Value,
    responses((status = 200, description = "Destroy started (job info)"))
))]
pub(crate) async fn destroy_deployment_from_body(
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    if body.get("payload").is_some() || body.get("variables").is_some() {
        return run_prepared_deployment_action(headers, body, "destroy").await;
    }

    let (project, region, environment, deployment_id) = match deployment_selector_from_body(&body) {
        Ok(selector) => selector,
        Err(e) => return handle_result(Err(e)).await.into_response(),
    };

    deployment_action_from_existing(
        headers,
        project,
        region,
        environment,
        deployment_id,
        "destroy",
    )
    .await
}

async fn reapply_deployment(
    headers: HeaderMap,
    project: String,
    region: String,
    environment: String,
    deployment_id: String,
) -> Response {
    if let Err(e) = ensure_access(&headers, &project).await {
        return e.into_response();
    }

    let workload = project.clone();
    handlers::with_workload_account(
        workload,
        reapply_deployment_authorized(project, region, environment, deployment_id),
    )
    .await
}

async fn deployment_action_from_existing(
    headers: HeaderMap,
    project: String,
    region: String,
    environment: String,
    deployment_id: String,
    command: &'static str,
) -> Response {
    if let Err(e) = ensure_access(&headers, &project).await {
        return e.into_response();
    }

    let workload = project.clone();
    handlers::with_workload_account(
        workload,
        deployment_action_from_existing_authorized(
            project,
            region,
            environment,
            deployment_id,
            command,
        ),
    )
    .await
}

async fn load_deployment(
    project: &str,
    region: &str,
    environment: &str,
    deployment_id: &str,
) -> Result<env_defs::DeploymentResp, anyhow::Error> {
    let deployment_value = handlers::describe_deployment(&json!({
        "project": project,
        "region": region,
        "environment": environment,
        "deployment_id": deployment_id
    }))
    .await?;

    serde_json::from_value(deployment_value)
        .map_err(|e| anyhow::anyhow!("Invalid deployment record: {}", e))
}

async fn deployment_action_from_existing_authorized(
    project: String,
    region: String,
    environment: String,
    deployment_id: String,
    command: &'static str,
) -> Response {
    use env_common::interface::GenericCloudHandler;
    use env_defs::{ApiInfraPayload, ApiInfraPayloadWithVariables, CloudProvider, ExtraData};

    let deployment = match load_deployment(&project, &region, &environment, &deployment_id).await {
        Ok(deployment) => deployment,
        Err(e) => return handle_result(Err(e)).await.into_response(),
    };

    let handler = GenericCloudHandler::workload(&deployment.project_id, &deployment.region).await;
    let payload = ApiInfraPayload {
        command: command.to_string(),
        flags: vec![],
        module: deployment.module.to_lowercase(),
        module_version: deployment.module_version.clone(),
        module_type: deployment.module_type.clone(),
        module_track: deployment.module_track.clone(),
        name: String::new(),
        environment: deployment.environment.clone(),
        deployment_id: deployment.deployment_id.clone(),
        project_id: deployment.project_id.clone(),
        region: deployment.region.clone(),
        drift_detection: deployment.drift_detection.clone(),
        next_drift_check_epoch: -1,
        annotations: serde_json::json!({}),
        dependencies: deployment.dependencies.clone(),
        initiated_by: handler.get_user_id().await.unwrap_or("api".into()),
        cpu: deployment.cpu.clone(),
        memory: deployment.memory.clone(),
        reference: deployment.reference.clone(),
        extra_data: ExtraData::None,
    };

    let payload_with_variables = ApiInfraPayloadWithVariables {
        payload,
        variables: deployment.variables.clone(),
    };
    let payload_value = match serde_json::to_value(&payload_with_variables.payload) {
        Ok(value) => value,
        Err(e) => {
            return handle_result(Err(anyhow::anyhow!(
                "Failed to serialize {} payload: {}",
                command,
                e
            )))
            .await
            .into_response()
        }
    };

    run_claim_authorized(
        payload_value,
        payload_with_variables.variables,
        payload_with_variables.payload,
    )
    .await
}

async fn reapply_deployment_authorized(
    project: String,
    region: String,
    environment: String,
    deployment_id: String,
) -> Response {
    use env_common::interface::GenericCloudHandler;
    use env_common::logic::validate_and_prepare_claim;
    use env_defs::{DeploymentResp, ExtraData, ModuleResp};

    let deployment: DeploymentResp =
        match load_deployment(&project, &region, &environment, &deployment_id).await {
            Ok(deployment) => deployment,
            Err(e) => return handle_result(Err(e)).await.into_response(),
        };

    let module_track = deployment.module_track.clone();
    let module_name = deployment.module.clone();
    let module_version = deployment.module_version.clone();
    let module_value = match deployment.module_type.as_str() {
        "stack" => {
            handlers::get_stack_version(&json!({
                "track": module_track,
                "stack_name": module_name,
                "stack_version": module_version
            }))
            .await
        }
        "module" => {
            handlers::get_module_version(&json!({
                "track": module_track,
                "module_name": module_name,
                "module_version": module_version
            }))
            .await
        }
        other => Err(anyhow::anyhow!("Unsupported module type: {}", other)),
    };

    let module_value = match module_value {
        Ok(value) => value,
        Err(e) => return handle_result(Err(e)).await.into_response(),
    };

    let module: ModuleResp = match serde_json::from_value(module_value) {
        Ok(module) => module,
        Err(e) => {
            return handle_result(Err(anyhow::anyhow!("Invalid module record: {}", e)))
                .await
                .into_response()
        }
    };

    let claim_yaml = env_utils::generate_deployment_claim(&deployment, &module);
    let yaml: serde_yaml::Value = match serde_yaml::from_str(&claim_yaml) {
        Ok(yaml) => yaml,
        Err(e) => {
            return handle_result(Err(anyhow::anyhow!(
                "Failed to regenerate deployment claim: {}",
                e
            )))
            .await
            .into_response()
        }
    };

    let handler = GenericCloudHandler::workload(&deployment.project_id, &deployment.region).await;
    let reference_fallback = if deployment.reference.is_empty() {
        "api-reapply"
    } else {
        &deployment.reference
    };

    let (_, payload_with_variables) = match validate_and_prepare_claim(
        &handler,
        &yaml,
        &deployment.environment,
        "apply",
        vec![],
        ExtraData::None,
        reference_fallback,
    )
    .await
    {
        Ok(prepared) => prepared,
        Err(e) => return handle_result(Err(e)).await.into_response(),
    };

    let payload_value = match serde_json::to_value(&payload_with_variables.payload) {
        Ok(value) => value,
        Err(e) => {
            return handle_result(Err(anyhow::anyhow!(
                "Failed to serialize reapply payload: {}",
                e
            )))
            .await
            .into_response()
        }
    };

    run_claim_authorized(
        payload_value,
        payload_with_variables.variables,
        payload_with_variables.payload,
    )
    .await
}

async fn run_prepared_deployment_action(
    headers: HeaderMap,
    body: Value,
    expected_command: &str,
) -> Response {
    log::info!("Received prepared deployment {} request", expected_command);

    let payload_value = match body.get("payload") {
        Some(p) => p.clone(),
        None => {
            return handle_result(Err(bad_request("Missing 'payload' field")))
                .await
                .into_response()
        }
    };

    let variables = match body.get("variables") {
        Some(v) => v.clone(),
        None => {
            return handle_result(Err(bad_request("Missing 'variables' field")))
                .await
                .into_response()
        }
    };

    let payload: env_defs::ApiInfraPayload = match serde_json::from_value(payload_value.clone()) {
        Ok(p) => p,
        Err(e) => {
            return handle_result(Err(bad_request(format!("Invalid payload: {}", e))))
                .await
                .into_response()
        }
    };

    if payload.command != expected_command {
        return handle_result(Err(bad_request(format!(
            "Payload command '{}' does not match endpoint command '{}'",
            payload.command, expected_command
        ))))
        .await
        .into_response();
    }

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
    use env_common::interface::GenericCloudHandler;
    use env_common::logic::is_deployment_in_progress;

    let handler = GenericCloudHandler::workload(&payload.project_id, &payload.region).await;
    let (in_progress, job_id, _, _) = is_deployment_in_progress(
        &handler,
        &payload.deployment_id,
        &payload.environment,
        true,
        false,
    )
    .await;
    if in_progress {
        return handle_result(Err(conflict(format!(
            "Deployment '{}' in environment '{}' already has job '{}' in progress",
            payload.deployment_id, payload.environment, job_id
        ))))
        .await
        .into_response();
    }

    let result = handlers::start_runner(&json!({
        "data": payload_value
    }))
    .await;

    let runner_response = match result {
        Ok(resp) => resp,
        Err(e) => return handle_result(Err(e)).await.into_response(),
    };

    let task_arn = runner_response["task_arn"]
        .as_str()
        .unwrap_or("")
        .to_string();
    let job_id = runner_response["job_id"]
        .as_str()
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| task_arn.split('/').last().unwrap_or(&task_arn).to_string());

    if job_id.is_empty() {
        return handle_result(Err(anyhow::anyhow!(
            "Runner response did not include a job identifier"
        )))
        .await
        .into_response();
    }

    log::info!("Task ARN: {}, Job ID: {}", task_arn, job_id);

    if let Err(e) = insert_deployment_record(&handler, &payload, &variables, &job_id).await {
        log::error!("Failed to insert deployment record: {}", e);
        return handle_result(Err(e)).await.into_response();
    }

    handle_result(Ok(json!({
        "task_arn": task_arn,
        "job_id": job_id,
        "deployment_id": payload.deployment_id,
        "environment": payload.environment,
        "project_id": payload.project_id,
        "region": payload.region
    })))
    .await
    .into_response()
}

async fn insert_deployment_record(
    handler: &env_common::interface::GenericCloudHandler,
    payload: &env_defs::ApiInfraPayload,
    variables: &serde_json::Value,
    job_id: &str,
) -> Result<(), anyhow::Error> {
    let payload_with_variables = env_defs::ApiInfraPayloadWithVariables {
        payload: payload.clone(),
        variables: variables.clone(),
    };

    env_common::insert_request_event(handler, &payload_with_variables, job_id).await
}
