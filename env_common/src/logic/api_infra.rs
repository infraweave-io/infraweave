use env_defs::{
    ApiInfraPayload, ApiInfraPayloadWithVariables, CloudHandlerError, CloudProvider, Dependency,
    DeploymentManifest, DeploymentResp, DriftDetection, ExtraData, GenericFunctionResponse,
    Webhook,
};
use env_utils::{
    convert_first_level_keys_to_snake_case, flatten_and_convert_first_level_keys_to_snake_case,
    get_version_track, verify_required_variables_are_set, verify_variable_claim_casing,
    verify_variable_existence_and_type,
};
use log::{debug, error, info, warn};

use crate::{interface::GenericCloudHandler, DeploymentStatusHandler};

pub async fn mutate_infra(
    handler: &GenericCloudHandler,
    payload: ApiInfraPayload,
) -> Result<GenericFunctionResponse, anyhow::Error> {
    let payload_value = serde_json::to_value(&payload)?;
    let event_payload = env_defs::start_runner_event(&payload_value);

    match handler.run_function(&event_payload).await {
        Ok(resp) => Ok(resp),
        Err(e) => Err(anyhow::anyhow!("Failed to run mutate_infra: {}", e)),
    }
}

pub fn get_deployment_details(
    environment: &str,
    deployment_manifest: DeploymentManifest,
) -> Result<(String, String, String, String, String), anyhow::Error> {
    let kind = deployment_manifest.kind;
    let region = deployment_manifest.spec.region;
    let module = kind.to_lowercase();
    let name = deployment_manifest.metadata.name;
    let deployment_id = format!("{}/{}", module, name);

    let environment_parts: Vec<&str> = environment.split('/').collect();
    // The parts should be <launcher>/<namespace>, where launcher is set by code and namespace is set by user
    let namespace = environment_parts[1..].join("/");
    validate_name(&namespace.to_string())?;
    let environment = environment.to_string();

    Ok((region, environment, deployment_id, module, name))
}

/// Validates and prepares a claim payload for deployment
/// This function performs all validation and constructs the payload without submitting it
pub async fn validate_and_prepare_claim(
    handler: &GenericCloudHandler,
    yaml: &serde_yaml::Value,
    environment: &str,
    command: &str,
    flags: Vec<String>,
    extra_data: ExtraData,
    reference_fallback: &str,
) -> Result<(String, ApiInfraPayloadWithVariables), anyhow::Error> {
    let api_version = yaml["apiVersion"].as_str().unwrap_or("").to_string();
    if api_version != "infraweave.io/v1" {
        error!("Not a supported InfraWeave API version: {}", api_version);
        return Err(anyhow::anyhow!("Unsupported API version: {}", api_version));
    }
    let deployment_manifest: DeploymentManifest = serde_yaml::from_value(yaml.clone())
        .expect("Failed to parse claim YAML to DeploymentManifest"); // TODO: Propagate error

    let claim = deployment_manifest.clone();

    let project_id = handler.get_project_id().to_string();

    let (region, environment, deployment_id, module, name) =
        get_deployment_details(environment, deployment_manifest.clone())?;

    let drift_detection_interval = match &deployment_manifest.spec.drift_detection {
        Some(drift_detection) => drift_detection.interval.to_string(),
        None => env_defs::DEFAULT_DRIFT_DETECTION_INTERVAL.to_string(),
    };
    let drift_detection_enabled = match &deployment_manifest.spec.drift_detection {
        Some(drift_detection) => drift_detection.enabled,
        None => false,
    };
    let drift_detection_auto_remediate = match &deployment_manifest.spec.drift_detection {
        Some(drift_detection) => drift_detection.auto_remediate,
        None => false,
    };
    let drift_detection_webhooks: Vec<Webhook> = match &deployment_manifest.spec.drift_detection {
        Some(drift_detection) => drift_detection.webhooks.clone(),
        None => vec![],
    };

    let drift_detection: DriftDetection = if deployment_manifest.spec.drift_detection.is_none() {
        serde_json::from_value(serde_json::json!({})).unwrap()
    } else {
        DriftDetection {
            interval: drift_detection_interval,
            enabled: drift_detection_enabled,
            auto_remediate: drift_detection_auto_remediate,
            webhooks: drift_detection_webhooks,
        }
    };

    let deployment_variables: serde_yaml::Mapping = deployment_manifest.spec.variables;
    let provided_variables: serde_json::Value = if deployment_variables.is_empty() {
        serde_json::json!({})
    } else {
        serde_json::to_value(&deployment_variables)?
    };
    let is_stack = match (
        deployment_manifest.spec.module_version.is_some(),
        deployment_manifest.spec.stack_version.is_some(),
    ) {
        (true, false) => false,
        (false, true) => true,
        (true, true) => {
            error!("Both moduleVersion and stackVersion are set, only one should be set");
            return Err(anyhow::anyhow!(
                "Both moduleVersion and stackVersion are set, only one should be set"
            ));
        }
        (false, false) => {
            error!("Neither moduleVersion nor stackVersion are set, one should be set");
            return Err(anyhow::anyhow!(
                "Neither moduleVersion nor stackVersion are set, one should be set"
            ));
        }
    };

    let dependencies: Vec<Dependency> = match deployment_manifest.spec.dependencies {
        None => vec![],
        Some(dependencies) => dependencies
            .iter()
            .map(|d| Dependency {
                project_id: project_id.to_string(),
                region: region.to_string(),
                deployment_id: format!(
                    "{}/{}",
                    d.deployment_id.to_lowercase(),
                    d.environment.to_lowercase()
                ),
                environment: environment.clone(),
            })
            .collect(),
    };

    let module_version = match is_stack {
        true => deployment_manifest.spec.stack_version.clone().unwrap(),
        false => deployment_manifest.spec.module_version.clone().unwrap(),
    };

    let reference = match deployment_manifest.spec.reference {
        None => reference_fallback.to_string(),
        Some(reference) => reference,
    };

    let annotations: serde_json::Value =
        serde_json::to_value(&deployment_manifest.metadata.annotations)
            .map_err(|e| anyhow::anyhow!("Failed to convert annotations YAML to JSON: {}", e))?;

    let track = match get_version_track(&module_version) {
        Ok(track) => track,
        Err(e) => {
            error!("Failed to get track from version: {}", e);
            return Err(anyhow::anyhow!("Failed to get track from version: {}", e));
        }
    };

    let module_resp = match if is_stack {
        debug!("Verifying if stack version exists: {}", module);
        handler
            .get_stack_version(&module, &track, &module_version)
            .await
    } else {
        debug!("Verifying if module version exists: {}", module);
        handler
            .get_module_version(&module, &track, &module_version)
            .await
    } {
        Ok(module) => match module {
            Some(module_resp) => module_resp,
            None => {
                return Err(anyhow::anyhow!(
                    "{} version does not exist: {}",
                    if is_stack { "Stack" } else { "Module" },
                    module_version
                ));
            }
        },
        Err(e) => {
            return Err(anyhow::anyhow!(
                "Failed to verify {} version: {}",
                if is_stack { "Stack" } else { "Module" },
                e
            ));
        }
    };

    validate_kind(&deployment_manifest.kind, &module_resp.module_name)?;
    // Check if the module is deprecated - allow existing deployments to continue, but block new ones
    check_module_deprecation(
        handler,
        &module_resp,
        is_stack,
        &module,
        &module_version,
        &deployment_id,
        &environment,
    )
    .await?;

    let variables = if is_stack {
        let dont_flatten: Vec<&String> = module_resp
            .tf_providers
            .iter()
            .flat_map(|p| p.tf_variables.iter().map(|v| &v.name))
            .collect();
        flatten_and_convert_first_level_keys_to_snake_case(&provided_variables, "", dont_flatten)
    } else {
        convert_first_level_keys_to_snake_case(&provided_variables)
    };

    // Validate input according to module schema
    verify_variable_existence_and_type(&module_resp, &variables)?;

    // Verify that all required variables are set
    verify_required_variables_are_set(&module_resp, &variables)?;

    // Verify that all provided claim variables are in camelCase and not in snake_case
    verify_variable_claim_casing(&claim, &provided_variables)?;

    info!("Validated claim for environment: {}", environment);
    info!("command: {}", command);
    info!("module: {}", module);
    info!("module_version: {}", module_version);
    info!("name: {}", name);
    info!("environment: {}", environment);
    info!("variables: {}", variables);
    info!("annotations: {}", annotations);
    info!("dependencies: {:?}", dependencies);

    let payload = ApiInfraPayload {
        command: command.to_string(),
        flags: flags.clone(),
        module: module.clone().to_lowercase(), // TODO: Only have access to kind, not the module name (which is assumed to be lowercase of module_name)
        module_type: if is_stack { "stack" } else { "module" }.to_string(),
        module_version: module_version.clone(),
        module_track: track,
        name: name.clone(),
        environment: environment.clone(),
        deployment_id: deployment_id.clone(),
        project_id: project_id.to_string(),
        region: region.to_string(),
        drift_detection,
        next_drift_check_epoch: -1, // Prevent reconciler from finding this deployment since it is in progress
        annotations,
        dependencies,
        initiated_by: handler.get_user_id().await.unwrap(),
        cpu: module_resp.cpu.clone(),
        memory: module_resp.memory.clone(),
        reference: reference.clone(),
        extra_data,
    };

    let payload_with_variables = ApiInfraPayloadWithVariables {
        payload: payload,
        variables: variables,
    };

    Ok((deployment_id, payload_with_variables))
}

pub async fn run_claim(
    handler: &GenericCloudHandler,
    yaml: &serde_yaml::Value,
    environment: &str,
    command: &str,
    flags: Vec<String>,
    extra_data: ExtraData,
    reference_fallback: &str,
) -> Result<(String, String, ApiInfraPayloadWithVariables), anyhow::Error> {
    let (deployment_id, payload_with_variables) = validate_and_prepare_claim(
        handler,
        yaml,
        environment,
        command,
        flags,
        extra_data,
        reference_fallback,
    )
    .await?;

    let job_id = submit_claim_job(handler, &payload_with_variables).await?;

    Ok((job_id, deployment_id, payload_with_variables))
}

fn validate_kind(kind: &str, module_name: &str) -> Result<(), anyhow::Error> {
    if module_name != kind {
        let error_msg = match module_name.to_lowercase() == kind.to_lowercase() {
            true => format!("Kind {kind} not exist, did you mean {module_name}?"),
            false => format!("Kind {kind} does not exist"),
        };
        return Err(anyhow::anyhow!("{}", error_msg));
    }
    Ok(())
}

fn validate_name(name: &str) -> Result<(), anyhow::Error> {
    // Only a-z, 0-9, and -
    // Starts/ends with alphanumeric
    // Length between 1 and 63 characters
    let re = regex::Regex::new(r"^[a-z0-9](?:[-a-z0-9]{0,61}[a-z0-9])?$").unwrap();
    if !re.is_match(name) {
        error!("Deployment name and namespace ({}) must be 1-63 characters long, contain only lowercase letters (a-z), digits (0-9), or hyphens (-), and must start and end with a lowercase letter or digit.", name);
        return Err(anyhow::anyhow!(
            "Deployment name and namespace ({}) must be 1-63 characters long, contain only lowercase letters (a-z), digits (0-9), or hyphens (-), and must start and end with a lowercase letter or digit.", name
        ));
    }
    Ok(())
}

pub async fn destroy_infra(
    handler: &GenericCloudHandler,
    deployment_id: &str,
    environment: &str,
    extra_data: ExtraData,
    override_version: Option<&str>,
) -> Result<String, anyhow::Error> {
    let name = "".to_string();
    match handler
        .get_deployment(deployment_id, environment, false)
        .await
    {
        Ok(deployment_resp) => match deployment_resp {
            Some(deployment) => {
                println!("Deployment exists");
                let command = "destroy".to_string();
                let module = deployment.module;
                // let name = deployment.name;
                let environment = deployment.environment;
                let variables: serde_json::Value =
                    serde_json::to_value(&deployment.variables).unwrap();
                let drift_detection = deployment.drift_detection;
                let annotations: serde_json::Value = serde_json::from_str("{}").unwrap();
                let dependencies = deployment.dependencies;

                let module_version = match override_version {
                    Some(override_version) => {
                        verify_module_version(handler, &module, override_version).await?;
                        override_version.to_string()
                    }
                    None => deployment.module_version.clone(),
                };

                info!("Tearing down deployment: {}", deployment_id);
                info!("command: {}", command);
                // info!("module: {}", module);
                // info!("name: {}", name);
                // info!("environment: {}", environment);
                info!("variables: {}", variables);
                info!("annotations: {}", annotations);
                info!("dependencies: {:?}", dependencies);

                let payload = ApiInfraPayload {
                    command: command.clone(),
                    flags: vec![],
                    module: module.clone().to_lowercase(), // TODO: Only have access to kind, not the module name (which is assumed to be lowercase of module_name)
                    module_version: module_version.clone(),
                    module_type: deployment.module_type.clone(),
                    module_track: deployment.module_track.clone(),
                    name: name.clone(),
                    environment: environment.clone(),
                    deployment_id: deployment_id.to_string(),
                    project_id: deployment.project_id.clone(),
                    region: deployment.region.clone(),
                    drift_detection,
                    next_drift_check_epoch: -1, // Prevent reconciler from finding this deployment since it is in progress
                    annotations,
                    dependencies,
                    initiated_by: handler.get_user_id().await.unwrap(),
                    cpu: deployment.cpu,
                    memory: deployment.memory,
                    reference: deployment.reference,
                    extra_data,
                };

                let payload_with_variables = ApiInfraPayloadWithVariables {
                    payload: payload,
                    variables: variables,
                };

                let job_id: String = submit_claim_job(handler, &payload_with_variables).await?;
                Ok(job_id)
            }
            None => Err(anyhow::anyhow!(
                "Failed to describe deployment, deployment was not found"
            )),
        },
        Err(e) => Err(anyhow::anyhow!("Failed to describe deployment: {}", e)),
    }
}

async fn verify_module_version(
    handler: &GenericCloudHandler,
    module: &str,
    module_version: &str,
) -> Result<(), anyhow::Error> {
    info!("Verifying that version override exists: {}", module_version);
    let module_version_track = get_version_track(module_version)
        .map_err(|e| anyhow::anyhow!("Failed to get track from version: {}", e))?;
    match handler
        .get_module_version(module, &module_version_track, module_version)
        .await
    {
        Ok(Some(_)) => Ok(()),
        Ok(None) => {
            return Err(anyhow::anyhow!(
                "Module version {} does not exist",
                module_version
            ));
        }
        Err(e) => {
            return Err(anyhow::anyhow!("Failed to verify module version: {}", e));
        }
    }
}

pub async fn driftcheck_infra(
    handler: &GenericCloudHandler,
    deployment_id: &str,
    environment: &str,
    remediate: bool,
    extra_data: ExtraData,
) -> Result<String, anyhow::Error> {
    let name = "".to_string();
    match handler
        .get_deployment(deployment_id, environment, false)
        .await
    {
        Ok(deployment_resp) => match deployment_resp {
            Some(deployment) => {
                println!("Deployment exists");
                let module = deployment.module;
                // let name = deployment.name;
                let environment = deployment.environment;
                let variables: serde_json::Value =
                    serde_json::to_value(&deployment.variables).unwrap();
                let drift_detection = deployment.drift_detection;
                let annotations: serde_json::Value = serde_json::from_str("{}").unwrap();
                let dependencies = deployment.dependencies;
                let module_version = deployment.module_version;

                let flags = if remediate {
                    vec![]
                } else {
                    vec!["-refresh-only".to_string()]
                };
                let command = if remediate { "apply" } else { "plan" };

                info!("Driftcheck deployment: {}", deployment_id);
                info!("command: {}", &command);
                // info!("module: {}", module);
                // info!("name: {}", name);
                // info!("environment: {}", environment);
                info!("variables: {}", variables);
                info!("annotations: {}", annotations);
                info!("dependencies: {:?}", dependencies);

                let payload = ApiInfraPayload {
                    command: command.to_string(),
                    flags: flags.clone(),
                    module: module.clone().to_lowercase(), // TODO: Only have access to kind, not the module name (which is assumed to be lowercase of module_name)
                    module_version: module_version.clone(),
                    module_type: deployment.module_type.clone(),
                    module_track: deployment.module_track.clone(),
                    name: name.clone(),
                    environment: environment.clone(),
                    deployment_id: deployment_id.to_string(),
                    project_id: deployment.project_id.clone(),
                    region: deployment.region.clone(),
                    drift_detection,
                    next_drift_check_epoch: -1, // Prevent reconciler from finding this deployment since it is in progress
                    annotations,
                    dependencies,
                    initiated_by: if remediate {
                        handler.get_user_id().await.unwrap()
                    } else {
                        deployment.initiated_by.clone()
                    }, // Dont change the user if it's only a drift check
                    cpu: deployment.cpu.clone(),
                    memory: deployment.memory.clone(),
                    reference: deployment.reference.clone(),
                    extra_data,
                };

                let payload_with_variables = ApiInfraPayloadWithVariables {
                    payload: payload,
                    variables: variables,
                };

                let job_id: String = submit_claim_job(handler, &payload_with_variables).await?;
                Ok(job_id)
            }
            None => Err(anyhow::anyhow!(
                "Failed to describe deployment, deployment was not found"
            )),
        },
        Err(e) => Err(anyhow::anyhow!("Failed to describe deployment: {}", e)),
    }
}

pub async fn submit_claim_job(
    handler: &GenericCloudHandler,
    payload_with_variables: &ApiInfraPayloadWithVariables,
) -> Result<String, anyhow::Error> {
    let payload = &payload_with_variables.payload;
    let (in_progress, job_id, _, _) = is_deployment_in_progress(
        handler,
        &payload.deployment_id,
        &payload.environment,
        true,
        false,
    )
    .await;
    if in_progress {
        return Err(CloudHandlerError::JobAlreadyInProgress(job_id).into());
    }

    // HTTP mode: send claim to API endpoint
    if env_aws::is_http_mode_enabled() {
        info!("HTTP mode: Sending claim to API endpoint");

        // Validate project_id is set for deployment operations
        if payload.project_id == "http-mode-no-project" {
            return Err(anyhow::anyhow!(
                "Project ID is required for deployment operations in HTTP mode.\n\
                Use --project flag or set INFRAWEAVE_PROJECT_ID=<aws-account-id>"
            ));
        }

        // Send ApiInfraPayloadWithVariables to server
        // Server will insert deployment record and launch ECS task with ApiInfraPayload only (no variables)
        let response = env_aws::http_post(
            "/api/v1/claim/run",
            &serde_json::to_value(payload_with_variables)?,
        )
        .await
        .map_err(|e| anyhow::anyhow!("Failed to submit claim via HTTP: {}", e))?;

        let job_id = response["job_id"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("No job_id in response"))?
            .to_string();

        info!("Claim submitted via HTTP, job_id: {}", job_id);

        // Server handles event/deployment insertion
        return Ok(job_id);
    }

    let job_id: String = match mutate_infra(handler, payload.clone()).await {
        Ok(resp) => {
            info!("Request successfully submitted");
            let job_id = resp.payload["job_id"].as_str().unwrap().to_string();
            job_id
        }
        Err(e) => {
            let error_text = e.to_string();
            error!("Failed to deploy claim: {}", &error_text);
            return Err(anyhow::anyhow!("Failed to deploy claim: {}", &error_text));
        }
    };

    insert_request_event(handler, payload_with_variables, &job_id).await?;

    Ok(job_id)
}

pub async fn insert_request_event(
    handler: &GenericCloudHandler,
    payload_with_variables: &ApiInfraPayloadWithVariables,
    job_id: &str,
) -> Result<(), anyhow::Error> {
    let payload = &payload_with_variables.payload;
    let status_handler = DeploymentStatusHandler::new(
        &payload.command,
        &payload.module,
        &payload.module_version,
        &payload.module_type,
        &payload.module_track,
        "requested".to_string(),
        &payload.environment,
        &payload.deployment_id,
        &payload.project_id,
        &payload.region,
        "".to_string(),
        job_id.to_string(),
        &payload.name,
        payload_with_variables.variables.clone(),
        payload.drift_detection.clone(),
        payload.next_drift_check_epoch,
        payload.dependencies.clone(),
        serde_json::Value::Null,
        vec![],
        payload.initiated_by.as_str(),
        payload.cpu.clone(),
        payload.memory.clone(),
        payload.reference.clone(),
    );
    status_handler.send_event(handler).await;
    status_handler.send_deployment(handler).await?;
    Ok(())
}

pub async fn is_deployment_in_progress(
    handler: &GenericCloudHandler,
    deployment_id: &str,
    environment: &str,
    job_check: bool, // Ensure that the job is actually running even if deployment status is in progress
    include_deleted: bool,
) -> (bool, String, String, Option<DeploymentResp>) {
    let busy_statuses = ["requested", "initiated"]; // TODO: use enums

    let deployment = match handler
        .get_deployment(deployment_id, environment, include_deleted)
        .await
    {
        Ok(deployment_resp) => match deployment_resp {
            Some(deployment) => deployment,
            None => {
                info!(
                    "No existing deployment was not found for {}, {}",
                    deployment_id, environment
                );
                return (false, "".to_string(), "".to_string(), None);
            }
        },
        Err(e) => {
            error!("Failed to describe deployment: {}", e);
            return (false, "".to_string(), "".to_string(), None);
        }
    };

    if busy_statuses.contains(&deployment.status.as_str()) {
        if job_check {
            warn!(
                "Deployment is currently in process according to deployment: {}",
                deployment.status
            );
            warn!(
                "Trying to verify that a VM is running for deployment job: {}",
                deployment.job_id
            );
            match handler.get_job_status(&deployment.job_id).await {
                Ok(Some(job_status)) => {
                    if job_status.is_running {
                        warn!("Job {} is indeed running", deployment.job_id);
                    } else {
                        warn!("Job {} is not running, proceeding. (This may have been caused by an error in the previous run)", deployment.job_id);
                        return (
                            false,
                            "".to_string(),
                            deployment.status.to_string(),
                            Some(deployment.clone()),
                        );
                    }
                }
                Ok(None) => {
                    error!(
                        "No job status found for {}, please talk to your administrator.",
                        deployment.job_id
                    );
                }
                Err(e) => {
                    error!("Failed to get job status for {}: {}", deployment.job_id, e);
                }
            };
        }
        return (
            true,
            deployment.job_id.clone(),
            deployment.status.to_string(),
            Some(deployment.clone()),
        );
    }

    (
        false,
        "".to_string(),
        deployment.status.to_string(),
        Some(deployment.clone()),
    )
}

pub async fn is_deployment_plan_in_progress(
    handler: &GenericCloudHandler,
    deployment_id: &str,
    environment: &str,
    job_id: &str,
) -> (bool, String, Option<DeploymentResp>) {
    // Ensure we use the task ID, not the full ARN, for DB lookups
    let job_id_short = job_id.split('/').last().unwrap_or(job_id);

    // In HTTP mode, check actual ECS task status first
    if env_aws::is_http_mode_enabled() {
        let project_id = handler.get_project_id();
        let region = handler.get_region();

        if let Ok(job_status) = env_aws::http_get_job_status(project_id, region, job_id_short).await
        {
            let status = job_status
                .get("status")
                .and_then(|v| v.as_str())
                .unwrap_or("UNKNOWN");
            let stopped_reason = job_status
                .get("stopped_reason")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let exit_code = job_status
                .get("exit_code")
                .and_then(|v| v.as_i64())
                .unwrap_or(0);

            info!(
                "ECS task status: {}, stopped_reason: {}, exit_code: {}",
                status, stopped_reason, exit_code
            );

            // If task is stopped, it's not in progress anymore
            if status == "STOPPED" || status == "DEPROVISIONING" {
                if exit_code != 0 {
                    error!(
                        "ECS task failed with exit code {}: {}",
                        exit_code, stopped_reason
                    );
                } else if !stopped_reason.is_empty()
                    && stopped_reason != "Essential container in task exited"
                {
                    warn!("ECS task stopped with reason: {}", stopped_reason);
                }

                // Try to get deployment record if available
                let deployment = handler
                    .get_plan_deployment(deployment_id, environment, job_id_short)
                    .await
                    .ok()
                    .flatten();

                if let Some(ref d) = deployment {
                    info!(
                        "Found deployment plan record for job {}: status={}",
                        job_id_short, d.status
                    );
                } else {
                    warn!("Job {} finished with exit code {} but no deployment plan record found yet (PK=PLAN#{} SK={})", 
                        job_id_short, exit_code,
                        env_defs::get_deployment_identifier(project_id, region, deployment_id, environment),
                        job_id_short
                    );
                }

                return (false, job_id_short.to_string(), deployment);
            }
            // Task is still running/pending/provisioning
            else if status == "RUNNING"
                || status == "PENDING"
                || status == "PROVISIONING"
                || status == "ACTIVATING"
            {
                // Try to get deployment record if available
                let deployment = handler
                    .get_plan_deployment(deployment_id, environment, job_id_short)
                    .await
                    .ok()
                    .flatten();
                return (true, job_id_short.to_string(), deployment);
            }
        }
    }

    // Fall back to checking deployment record (Lambda mode or HTTP mode fallback)
    let busy_statuses = ["requested", "initiated"]; // TODO: use enums

    let deployment = match handler
        .get_plan_deployment(deployment_id, environment, job_id_short)
        .await
    {
        Ok(deployment_resp) => match deployment_resp {
            Some(deployment) => deployment,
            None => {
                // Deployment not found yet - job may still be starting
                info!(
                    "Deployment plan not found yet for {}, job is likely still starting",
                    deployment_id
                );
                return (true, job_id_short.to_string(), None);
            }
        },
        Err(e) => {
            error!("Failed to describe deployment: {}", e);
            return (false, "".to_string(), None);
        }
    };

    let in_progress = busy_statuses.contains(&deployment.status.as_str());
    let job_id = deployment.job_id.clone();

    (in_progress, job_id, Some(deployment.clone()))
}

/// Checks the progress of a standard deployment via ECS task status + DB record
pub async fn check_deployment_progress(
    handler: &GenericCloudHandler,
    deployment_id: &str,
    environment: &str,
    job_id: &str,
) -> (bool, String, Option<DeploymentResp>) {
    let job_id_short = job_id.split('/').last().unwrap_or(job_id);

    // In HTTP mode, check actual ECS task status first
    if env_aws::is_http_mode_enabled() {
        let project_id = handler.get_project_id();
        let region = handler.get_region();

        if let Ok(job_status) = env_aws::http_get_job_status(project_id, region, job_id_short).await
        {
            let status = job_status
                .get("status")
                .and_then(|v| v.as_str())
                .unwrap_or("UNKNOWN");
            let stopped_reason = job_status
                .get("stopped_reason")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let exit_code = job_status
                .get("exit_code")
                .and_then(|v| v.as_i64())
                .unwrap_or(0);

            // If task is stopped, it's not in progress anymore
            if status == "STOPPED" || status == "DEPROVISIONING" {
                if exit_code != 0 {
                    error!(
                        "ECS task failed with exit code {}: {}",
                        exit_code, stopped_reason
                    );
                } else if !stopped_reason.is_empty()
                    && stopped_reason != "Essential container in task exited"
                {
                    warn!("ECS task stopped with reason: {}", stopped_reason);
                }

                // Get final deployment status
                let deployment = handler
                    .get_deployment(deployment_id, environment, false)
                    .await
                    .ok()
                    .flatten();

                return (false, job_id_short.to_string(), deployment);
            }
            // Task is still running/pending/provisioning
            else if status == "RUNNING"
                || status == "PENDING"
                || status == "PROVISIONING"
                || status == "ACTIVATING"
            {
                // Return current state (likely still says in-progress in DB)
                let deployment = handler
                    .get_deployment(deployment_id, environment, false)
                    .await
                    .ok()
                    .flatten();
                return (true, job_id_short.to_string(), deployment);
            }
        }
    }

    // Fallback if not HTTP enabled or check failed
    let (in_progress, _, _status, deployment) =
        super::is_deployment_in_progress(handler, deployment_id, environment, false, false).await;

    // Warn if DB says in-progress but we might be stuck
    if in_progress {
        // We could double check job_id match but is_deployment_in_progress returns the job_id from DB
    }

    (in_progress, job_id_short.to_string(), deployment)
}

pub fn get_default_cpu() -> String {
    "1024".to_string() // 1 vCPU aws
}

pub fn get_default_memory() -> String {
    "2048".to_string() // 2 GB aws
}

/// Checks if a module/stack version is deprecated
///
/// Blocks new deployments from using deprecated modules, but allows existing deployments
/// to continue operating (with warnings) for updates, destroy, and drift checks.
pub async fn check_module_deprecation(
    handler: &GenericCloudHandler,
    module_resp: &env_defs::ModuleResp,
    is_stack: bool,
    module: &str,
    module_version: &str,
    deployment_id: &str,
    environment: &str,
) -> Result<(), anyhow::Error> {
    if !module_resp.deprecated {
        return Ok(());
    }

    let existing_deployment = handler
        .get_deployment(deployment_id, environment, false)
        .await?;

    match existing_deployment {
        Some(_) => {
            // Allow existing deployments to continue using deprecated modules
            warn!(
                "{} {} version {} is deprecated but allowing existing deployment {} to continue",
                if is_stack { "Stack" } else { "Module" },
                module,
                module_version,
                deployment_id
            );
            Ok(())
        }
        None => {
            // Prevent new deployments from using deprecated modules
            let mut error_msg = format!(
                "{} {} version {} has been deprecated and cannot be used for new deployments.",
                if is_stack { "Stack" } else { "Module" },
                module,
                module_version
            );

            if let Some(msg) = &module_resp.deprecated_message {
                error_msg.push_str(&format!("\nReason: {}", msg));
            }

            error_msg.push_str("\nPlease use a different version.");

            Err(anyhow::anyhow!(error_msg))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn test_claim_correct_casing() {
        let yaml_manifest = r#"
        apiVersion: infraweave.io/v1
        kind: S3Bucket
        metadata:
            name: bucket1a
        spec:
            region: eu-west-1
            moduleVersion: 0.0.21
            variables: {}
        "#;
        let deployment: Result<DeploymentManifest, serde_yaml::Error> =
            serde_yaml::from_str(yaml_manifest);
        let module_name = "S3Bucket";
        assert_eq!(deployment.is_ok(), true);
        // Deployment must be Ok here
        assert_eq!(
            validate_kind(&deployment.unwrap().kind, module_name).is_ok(),
            true
        );
    }

    #[test]
    fn test_claim_kind_incorrect_casing() {
        let yaml_manifest = r#"
        apiVersion: infraweave.io/v1
        kind: s3bucket
        metadata:
            name: bucket1a
        spec:
            region: eu-west-1
            moduleVersion: 0.0.21
            variables: {}
        "#;
        let deployment: Result<DeploymentManifest, serde_yaml::Error> =
            serde_yaml::from_str(yaml_manifest);
        let module_name = "S3Bucket";

        assert_eq!(deployment.is_ok(), true);
        // Deployment must be Ok here
        let res = validate_kind(&deployment.as_ref().unwrap().kind, module_name);
        assert_eq!(res.is_ok(), false);

        let err = res.unwrap_err().to_string();
        assert!(err.contains("did you mean"), "error was: {}", err);
    }

    #[test]
    fn test_claim_valid_deployment() {
        let yaml_manifest = r#"
    apiVersion: infraweave.io/v1
    kind: S3Bucket
    metadata:
        name: bucket1a
    spec:
        region: eu-west-1
        moduleVersion: 0.0.21
        variables: {}
    "#;

        let deployment: Result<DeploymentManifest, serde_yaml::Error> =
            serde_yaml::from_str(yaml_manifest);
        assert_eq!(deployment.is_ok(), true);
    }

    #[test]
    fn test_claim_missing_region() {
        let yaml_manifest = r#"
    apiVersion: infraweave.io/v1
    kind: S3Bucket
    metadata:
        name: bucket1a
    spec:
        moduleVersion: 0.0.21
        variables: {}
    "#;

        let deployment: Result<DeploymentManifest, serde_yaml::Error> =
            serde_yaml::from_str(yaml_manifest);
        assert_eq!(deployment.is_ok(), false);
    }
}
