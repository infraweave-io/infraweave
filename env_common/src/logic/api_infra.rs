use env_defs::{
    ApiInfraPayload, Dependency, DeploymentResp, DriftDetection, GenericFunctionResponse, Webhook,
};
use env_utils::{
    convert_first_level_keys_to_snake_case, flatten_and_convert_first_level_keys_to_snake_case,
    get_version_track,
};
use log::{debug, error, info};

use crate::{interface::CloudHandler, DeploymentStatusHandler};

use super::common::handler;

pub async fn mutate_infra(
    payload: ApiInfraPayload,
) -> Result<GenericFunctionResponse, anyhow::Error> {
    let payload = serde_json::json!({
        "event": "start_runner",
        "data": payload
    });

    match handler().run_function(&payload).await {
        Ok(resp) => Ok(resp),
        Err(e) => Err(anyhow::anyhow!("Failed to insert event: {}", e)),
    }
}

pub async fn run_claim(
    yaml: &serde_yaml::Value,
    environment: &str,
    command: &str,
) -> Result<(String, String), anyhow::Error> {
    let api_version = yaml["apiVersion"].as_str().unwrap_or("").to_string();
    if api_version != "infraweave.io/v1" {
        error!("Not a supported InfraWeave API version: {}", api_version);
        return Err(anyhow::anyhow!("Unsupported API version: {}", api_version));
    }
    let kind = yaml["kind"].as_str().unwrap().to_string();

    let handler = handler();
    let project_id = handler.get_project_id().to_string();
    let region = handler.get_region().to_string();

    let module = kind.to_lowercase();
    let name = yaml["metadata"]["name"].as_str().unwrap().to_string();
    let environment = environment.to_string();
    let deployment_id = format!("{}/{}", module, name);

    let drift_detection_interval = yaml["spec"]["driftDetection"]["interval"]
        .as_str()
        .unwrap_or(env_defs::DEFAULT_DRIFT_DETECTION_INTERVAL)
        .to_string();
    let drift_detection_enabled = yaml["spec"]["driftDetection"]["enabled"]
        .as_bool()
        .unwrap_or(false);
    let drift_detection_auto_remediate = yaml["spec"]["driftDetection"]["autoRemediate"]
        .as_bool()
        .unwrap_or(false);
    let drift_detection_webhooks: Vec<Webhook> = match yaml
        .get("spec")
        .and_then(|spec| spec.get("driftDetection"))
        .and_then(|drift_detection| drift_detection.get("webhooks"))
        .and_then(|webhooks| webhooks.as_sequence())
    {
        Some(sequence) => serde_yaml::from_value(serde_yaml::Value::Sequence(sequence.clone()))
            .unwrap_or_else(|_| vec![]),
        None => vec![], // If any part of the chain is missing or not a sequence, return an empty Vec
    };

    let drift_detection: DriftDetection = if yaml["spec"]["driftDetection"].is_null() {
        serde_json::from_str("{}").unwrap()
    } else {
        DriftDetection {
            interval: drift_detection_interval,
            enabled: drift_detection_enabled,
            auto_remediate: drift_detection_auto_remediate,
            webhooks: drift_detection_webhooks,
        }
    };

    let variables_yaml = &yaml["spec"]["variables"];
    let variables: serde_json::Value = if variables_yaml.is_null() {
        serde_json::json!({})
    } else {
        serde_json::to_value(variables_yaml.clone())
            .expect("Failed to convert spec.variables YAML to JSON")
    };
    // Check if stack or module using spec.moduleVersion (which otherwise is spec.stackVersion)
    // TODO: Parse using serde
    let is_stack = yaml["spec"]["moduleVersion"].is_null();
    let variables = if is_stack {
        flatten_and_convert_first_level_keys_to_snake_case(&variables, "")
    } else {
        convert_first_level_keys_to_snake_case(&variables)
    };
    let dependencies_yaml = &yaml["spec"]["dependencies"];
    let dependencies: Vec<Dependency> = if dependencies_yaml.is_null() {
        Vec::new()
    } else {
        dependencies_yaml
            .clone()
            .as_sequence()
            .unwrap()
            .iter()
            .map(|d| Dependency {
                project_id: project_id.to_string(),
                region: region.to_string(),
                deployment_id: format!(
                    "{}/{}",
                    d["kind"].as_str().unwrap().to_lowercase(),
                    d["name"].as_str().unwrap()
                ),
                environment: {
                    // use namespace if specified, otherwise use same as deployment as default
                    if let Some(namespace) = d.get("namespace").and_then(|n| n.as_str()) {
                        let mut env_parts = environment.split('/').collect::<Vec<&str>>();
                        if env_parts.len() == 2 {
                            env_parts[1] = namespace;
                            env_parts.join("/")
                        } else {
                            environment.clone()
                        }
                    } else {
                        environment.clone()
                    }
                },
            })
            .collect()
    };
    let version_key = if is_stack {
        "stackVersion"
    } else {
        "moduleVersion"
    };
    let module_version = yaml["spec"][version_key].as_str().unwrap().to_string();
    let annotations: serde_json::Value =
        serde_json::to_value(yaml["metadata"]["annotations"].clone())
            .expect("Failed to convert annotations YAML to JSON");

    let track = match get_version_track(&module_version) {
        Ok(track) => track,
        Err(e) => {
            error!("Failed to get track from version: {}", e);
            return Err(anyhow::anyhow!("Failed to get track from version: {}", e));
        }
    };

    let module_resp = match if is_stack {
        debug!("Verifying if module version exists: {}", module);
        handler
            .get_module_version(&module, &track, &module_version)
            .await
    } else {
        debug!("Verifying if stack version exists: {}", module);
        handler
            .get_stack_version(&module, &track, &module_version)
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

    info!("Applying claim to environment: {}", environment);
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
        args: vec![],
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
        variables,
        annotations,
        dependencies,
        initiated_by: handler.get_user_id().await.unwrap(),
        cpu: module_resp.cpu.clone(),
        memory: module_resp.memory.clone(),
    };

    let job_id = submit_claim_job(&payload).await;

    Ok((job_id, deployment_id))
}

pub async fn destroy_infra(
    deployment_id: &str,
    environment: &str,
) -> Result<String, anyhow::Error> {
    let name = "".to_string();
    match handler()
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
                let module_version = deployment.module_version;

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
                    args: vec![],
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
                    variables,
                    annotations,
                    dependencies,
                    initiated_by: handler().get_user_id().await.unwrap(),
                    cpu: deployment.cpu,
                    memory: deployment.memory,
                };

                let job_id: String = submit_claim_job(&payload).await;
                Ok(job_id)
            }
            None => Err(anyhow::anyhow!(
                "Failed to describe deployment, deployment was not found"
            )),
        },
        Err(e) => Err(anyhow::anyhow!("Failed to describe deployment: {}", e)),
    }
}

pub async fn driftcheck_infra(
    deployment_id: &str,
    environment: &str,
    remediate: bool,
) -> Result<String, anyhow::Error> {
    let name = "".to_string();
    match handler()
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

                let args = if remediate {
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
                    args,
                    module: module.clone().to_lowercase(), // TODO: Only have access to kind, not the module name (which is assumed to be lowercase of module_name)
                    module_version: module_version.clone(),
                    module_type: deployment.module_type.clone(),
                    module_track: deployment.module_track.clone(),
                    name: name.clone(),
                    environment: environment.clone(),
                    deployment_id: deployment_id.to_string(),
                    project_id: deployment.project_id.clone(),
                    region: deployment.region.clone(),
                    variables,
                    drift_detection,
                    next_drift_check_epoch: -1, // Prevent reconciler from finding this deployment since it is in progress
                    annotations,
                    dependencies,
                    initiated_by: if remediate {
                        handler().get_user_id().await.unwrap()
                    } else {
                        deployment.initiated_by.clone()
                    }, // Dont change the user if it's only a drift check
                    cpu: deployment.cpu.clone(),
                    memory: deployment.memory.clone(),
                };

                let job_id: String = submit_claim_job(&payload).await;
                Ok(job_id)
            }
            None => Err(anyhow::anyhow!(
                "Failed to describe deployment, deployment was not found"
            )),
        },
        Err(e) => Err(anyhow::anyhow!("Failed to describe deployment: {}", e)),
    }
}

pub async fn submit_claim_job(payload: &ApiInfraPayload) -> String {
    let (in_progress, job_id, _, _) =
        is_deployment_in_progress(&payload.deployment_id, &payload.environment).await;
    if in_progress {
        info!("Deployment already requested, skipping");
        println!("Deployment already requested, skipping");
        return job_id;
    }

    let job_id: String = match mutate_infra(payload.clone()).await {
        Ok(resp) => {
            info!("Request successfully submitted");
            let job_id = resp.payload["job_id"].as_str().unwrap().to_string();
            job_id
        }
        Err(e) => {
            let error_text = e.to_string();
            error!("Failed to deploy claim: {}", &error_text);
            panic!("Failed to deploy claim: {}", &error_text);
        }
    };

    insert_requested_event(payload, &job_id).await;

    job_id
}

async fn insert_requested_event(payload: &ApiInfraPayload, job_id: &str) {
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
        "",
        job_id,
        &payload.name,
        payload.variables.clone(),
        payload.drift_detection.clone(),
        payload.next_drift_check_epoch,
        payload.dependencies.clone(),
        serde_json::Value::Null,
        vec![],
        payload.initiated_by.as_str(),
        payload.cpu.clone(),
        payload.memory.clone(),
    );
    status_handler.send_event().await;
    status_handler.send_deployment().await;
}

pub async fn is_deployment_in_progress(
    deployment_id: &str,
    environment: &str,
) -> (bool, String, String, Option<DeploymentResp>) {
    let busy_statuses = ["requested", "initiated"]; // TODO: use enums

    let deployment = match handler()
        .get_deployment(deployment_id, environment, false)
        .await
    {
        Ok(deployment_resp) => match deployment_resp {
            Some(deployment) => deployment,
            None => {
                error!("Failed to describe deployment, deployment was not found");
                return (false, "".to_string(), "".to_string(), None);
            }
        },
        Err(e) => {
            error!("Failed to describe deployment: {}", e);
            return (false, "".to_string(), "".to_string(), None);
        }
    };

    if busy_statuses.contains(&deployment.status.as_str()) {
        info!("Deployment is currently in process: {}", deployment.status);
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
    deployment_id: &String,
    environment: &String,
    job_id: &str,
) -> (bool, String, Option<DeploymentResp>) {
    let busy_statuses = ["requested", "initiated"]; // TODO: use enums

    let deployment = match handler()
        .get_plan_deployment(deployment_id, environment, job_id)
        .await
    {
        Ok(deployment_resp) => match deployment_resp {
            Some(deployment) => deployment,
            None => panic!("Deployment plan could not describe since it was not found"),
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

pub fn get_default_cpu() -> String {
    "1024".to_string() // 1 vCPU aws
}

pub fn get_default_memory() -> String {
    "2048".to_string() // 2 GB aws
}
