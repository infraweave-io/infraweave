use env_defs::{ApiInfraPayload, Dependency, DeploymentResp, GenericFunctionResponse};
use env_utils::{convert_first_level_keys_to_snake_case, flatten_and_convert_first_level_keys_to_snake_case};
use log::{error, info};

use crate::{interface::CloudHandler, DeploymentStatusHandler};

use super::common::handler;

pub async fn mutate_infra(payload: ApiInfraPayload) -> Result<GenericFunctionResponse, anyhow::Error> {
    let payload = serde_json::json!({
        "event": "start_runner",
        "data": payload
    });

    match handler().run_function(&payload).await {
        Ok(resp) => Ok(resp),
        Err(e) => {
            Err(anyhow::anyhow!("Failed to insert event: {}", e))
        }
    }
}

pub async fn run_claim(yaml: &serde_yaml::Value, environment: &str, command: &str) -> Result<(String, String), anyhow::Error> {
    let kind = yaml["kind"].as_str().unwrap().to_string();

    let module = kind.to_lowercase();
    let name = yaml["metadata"]["name"].as_str().unwrap().to_string();
    let environment = environment.to_string();
    let deployment_id = format!("{}/{}", module, name);
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
    let version_key = if is_stack { "stackVersion" } else { "moduleVersion" };
    let module_version = yaml["spec"][version_key].as_str().unwrap().to_string();
    let annotations: serde_json::Value = serde_json::to_value(yaml["metadata"]["annotations"].clone())
        .expect("Failed to convert annotations YAML to JSON");

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
        module: module.clone().to_lowercase(), // TODO: Only have access to kind, not the module name (which is assumed to be lowercase of module_name)
        module_version: module_version.clone(),
        name: name.clone(),
        environment: environment.clone(),
        deployment_id: deployment_id.clone(),
        variables: variables,
        annotations: annotations,
        dependencies: dependencies,
    };

    let job_id = submit_claim_job(&payload).await;

    Ok((job_id, deployment_id))
}

pub async fn destroy_infra(deployment_id: &str, environment: &str) -> Result<String, anyhow::Error> {
    let name = "".to_string();
    match handler()
        .get_deployment(deployment_id, &environment)
        .await
    {
        Ok(deployment_resp) => 
            match deployment_resp {
                Some(deployment) => {
                    println!("Deployment exists");
                    let command = "destroy".to_string();
                    let module = deployment.module;
                    // let name = deployment.name;
                    let environment = deployment.environment;
                    let variables: serde_json::Value = serde_json::to_value(&deployment.variables).unwrap();
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
                        module: module.clone().to_lowercase(), // TODO: Only have access to kind, not the module name (which is assumed to be lowercase of module_name)
                        module_version: module_version.clone(),
                        name: name.clone(),
                        environment: environment.clone(),
                        deployment_id: deployment_id.to_string(),
                        variables: variables,
                        annotations: annotations,
                        dependencies: dependencies,
                    };

                    let job_id: String = submit_claim_job(&payload).await;
                    Ok(job_id)
                },
                None => {
                    Err(anyhow::anyhow!("Failed to describe deployment, deployment was not found"))
                }
            }
        Err(e) => {
            Err(anyhow::anyhow!("Failed to describe deployment: {}", e))
        }
    }
}


async fn submit_claim_job(
    payload: &ApiInfraPayload,
) -> String {
    let (in_progress, job_id, _) = is_deployment_in_progress(&payload.deployment_id, &payload.environment).await;
    if in_progress {
        info!("Deployment already requested, skipping");
        println!("Deployment already requested, skipping");
        return job_id;
    }

    let job_id: String  = match mutate_infra(payload.clone()).await {
        Ok(resp) => {
            info!("Request successfully submitted");
            println!("Request successfully submitted");
            let job_id = resp.payload["job_id"].as_str().unwrap().to_string();
            job_id
        }
        Err(e) => {
            let error_text = e.to_string();
            error!("Failed to deploy claim: {}", &error_text);
            panic!("Failed to deploy claim: {}", &error_text);
        }
    };

    insert_requested_event(&payload, &job_id).await;

    job_id
}

async fn insert_requested_event(payload: &ApiInfraPayload, job_id: &str) {
    let status_handler = DeploymentStatusHandler::new(
        &payload.command,
        &payload.module,
        &payload.module_version,
        "requested".to_string(),
        &payload.environment,
        &payload.deployment_id,
        "",
        &job_id,
        &payload.name,
        payload.variables.clone(),
        payload.dependencies.clone(),
        serde_json::Value::Null,
        vec![],
    );
    status_handler.send_event().await;
    status_handler.send_deployment().await;
}


pub async fn is_deployment_in_progress(deployment_id: &str, environment: &str) -> (bool, String, String) {
    let busy_statuses = vec!["requested", "initiated"]; // TODO: use enums

    let deployment =  match handler().get_deployment(deployment_id, environment).await {
        Ok(deployment_resp) => match deployment_resp {
            Some(deployment) => deployment,
            None => {
                error!("Failed to describe deployment, deployment was not found");
                return (false, "".to_string(), "".to_string());
            }
        }
        Err(e) => {
            error!("Failed to describe deployment: {}", e);
            return (false, "".to_string(), "".to_string());
        }
    };

    if busy_statuses.contains(&deployment.status.as_str()) {
        return (true, deployment.job_id, deployment.status.to_string());
    }

    (false, "".to_string(), deployment.status.to_string())
}

pub async fn is_deployment_plan_in_progress(deployment_id: &String, environment: &String, job_id: &str) -> (bool, String, Option<DeploymentResp>) {
    let busy_statuses = vec!["requested", "initiated"]; // TODO: use enums

    let deployment= match handler().get_plan_deployment(deployment_id, environment, job_id).await {
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