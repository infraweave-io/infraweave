mod module;
mod read;

use crate::module::get_module;
use anyhow::{anyhow, Result};
use env_common::interface::{initialize_project_id_and_region, GenericCloudHandler};
use env_common::logic::driftcheck_infra;
use env_common::DeploymentStatusHandler;
use env_defs::{ApiInfraPayload, CloudProvider, Dependency, Dependent, DeploymentResp};
use env_utils::setup_logging;
use futures::future::join_all;
use log::{error, info};
use module::download_module;
use serde_json::Value;
use std::env;
use std::process::exit;
use std::vec;
use terraform_runner::{
    get_initial_deployment, run_opa_policy_checks, store_backend_file, store_tf_vars_json,
    terraform_apply_destroy, terraform_init, terraform_output, terraform_plan, terraform_show,
    terraform_validate,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    setup_logging().expect("Failed to initialize logging.");
    initialize_project_id_and_region().await;

    let handler = GenericCloudHandler::default().await;
    setup_misc().await;

    let payload = get_payload();

    println!("Storing terraform variables in tf_vars.json...");
    store_tf_vars_json(&payload.variables);
    store_backend_file().await;

    println!("Read deployment id from environment variable...");

    let command = &payload.command;
    let refresh_only = payload.args.iter().any(|e| e == "-refresh-only");

    let initial_deployment = get_initial_deployment(&payload, &handler).await;

    // To reduce clutter, a DeploymentStatusHandler is used to handle the status updates
    // since we will be updating the status multiple times and only a few fields change each time
    let mut status_handler = initiate_deployment_status_handler(initial_deployment, &payload);
    let job_id = get_current_job_id(&handler, &mut status_handler).await;

    // Mark that the deployment has started
    if command == "plan" && refresh_only {
        status_handler.set_is_drift_check();
    }
    status_handler.send_event(&handler).await;
    status_handler.send_deployment(&handler).await;

    // Check if there are any dependencies that are not finished
    if command == "apply" {
        // Check if all dependencies have state = successful, if not, store "waiting-on-dependency" status and exit
        check_dependencies(&payload, &handler, &mut status_handler).await;
    } else if command == "destroy" {
        // Check if there are any deployments that is depending on this deployment, if so, store "has-dependants" status and exit
        check_dependants(&payload, &handler, &mut status_handler).await;
    }

    let module = get_module(&payload, &mut status_handler).await;

    download_module(&module.s3_key, "./").await;

    terraform_init(&payload, &handler, &mut status_handler).await;

    terraform_validate(&payload, &handler, &mut status_handler).await;

    let plan_output = terraform_plan(&payload, &handler, &mut status_handler).await;

    terraform_show(
        &payload,
        &job_id,
        &module,
        &plan_output,
        &handler,
        &mut status_handler,
    )
    .await;

    run_opa_policy_checks(&handler, &mut status_handler).await;

    if command == "apply" || command == "destroy" {
        terraform_apply_destroy(&payload, &handler, &mut status_handler).await;

        if command == "apply" {
            terraform_output(&payload, &handler, &mut status_handler).await;
        }
    } else if command == "plan" {
        status_handler.set_status("successful".to_string());
        status_handler.set_event_duration();
        status_handler.set_last_event_epoch(); // Reset the event duration timer for the next event
        status_handler.send_event(&handler).await;
        status_handler.send_deployment(&handler).await;
    }

    // if !dependents.is_empty() {
    //     _trigger_dependent_deployments(&dependents).await; // TODO: WIP: needs to launch with replaced variables
    // }

    println!("Done!");

    Ok(())
}

async fn _trigger_dependent_deployments(dependent_deployments: &Vec<Dependent>) {
    // Retrigger each deployment asynchronously to run them in parallel
    let dependent_deployment_runs = dependent_deployments.clone().into_iter().map(|dependent| {
        let deployment_id = dependent.dependent_id.clone();
        let environment = dependent.environment.clone();
        async move {
            println!(
                "Deploymentid: {}, environment: {}",
                deployment_id, environment
            );
            let remediate = true; // Always apply remediation for dependent deployments (=> terraform apply)
            let handler = GenericCloudHandler::default().await;
            match driftcheck_infra(&handler, &deployment_id, &environment, remediate).await {
                Ok(_) => {
                    info!("Successfully requested drift check");
                }
                Err(e) => {
                    error!("Failed to request drift check: {}", e);
                }
            }
        }
    });

    join_all(dependent_deployment_runs).await;

    info!(
        "Successfully retriggered dependent deployments {:?}",
        dependent_deployments
    );
}

fn get_payload() -> ApiInfraPayload {
    let payload_env = env::var("PAYLOAD").unwrap();
    let payload: ApiInfraPayload = match serde_json::from_str(&payload_env) {
        Ok(json) => json,
        Err(e) => {
            eprintln!(
                "Failed to parse env-var PAYLOAD as ApiInfraPayload: {:?}",
                e
            );
            std::process::exit(1);
        }
    };
    payload
}

// fn cat_file(filename: &str) {
//     println!("=== File content: {} ===", filename);
//     let output = std::process::Command::new("cat")
//         .arg(filename)
//         .output()
//         .expect("Failed to execute command");

//     println!("{}", String::from_utf8_lossy(&output.stdout));
// }

async fn check_dependency_status(dependency: &Dependency) -> Result<(), anyhow::Error> {
    println!("Checking dependency status...");
    let handler = GenericCloudHandler::default().await;
    match handler
        .get_deployment(&dependency.deployment_id, &dependency.environment, false)
        .await
    {
        Ok(deployment) => match deployment {
            Some(deployment) => {
                if deployment.status == "successful" {
                    Ok(())
                } else {
                    Err(anyhow!("Dependency not finished"))
                }
            }
            None => panic!("Deployment could not describe since it was not found"),
        },
        Err(e) => {
            println!("Error: {:?}", e);
            panic!("Error getting deployment status");
        }
    }
}

async fn get_current_job_id(
    handler: &GenericCloudHandler,
    status_handler: &mut DeploymentStatusHandler<'_>,
) -> String {
    match handler.get_current_job_id().await {
        Ok(id) => {
            status_handler.set_job_id(id.clone());
            id.clone()
        }
        Err(e) => {
            println!("Error getting current job id: {:?}", e);
            let status = "failed".to_string();
            status_handler.set_error_text(
                "The job failed to fetch the job id, please retry again.".to_string(),
            );
            status_handler.set_status(status);
            status_handler.set_event_duration();
            status_handler.send_event(handler).await;
            status_handler.send_deployment(handler).await;
            exit(1);
        }
    }
}

fn initiate_deployment_status_handler(
    initial_deployment: Option<DeploymentResp>,
    payload: &ApiInfraPayload,
) -> DeploymentStatusHandler {
    let command = &payload.command;
    let environment = &payload.environment;
    let deployment_id = &payload.deployment_id;
    let project_id = &payload.project_id;
    let region = &payload.region;
    let error_text = "".to_string();
    let status = "initiated".to_string(); // received, initiated, completed, failed
    let job_id = "unknown_jobid".to_string();
    let initiated_by = &payload.initiated_by;

    DeploymentStatusHandler::new(
        command,
        &payload.module,
        &payload.module_version,
        &payload.module_type,
        &payload.module_track,
        status,
        environment,
        deployment_id,
        project_id,
        region,
        error_text,
        job_id,
        &payload.name,
        payload.variables.clone(),
        payload.drift_detection.clone(),
        payload.next_drift_check_epoch,
        payload.dependencies.clone(),
        if initial_deployment.is_some() {
            initial_deployment.clone().unwrap().output
        } else {
            Value::Null
        },
        if let Some(deployment) = initial_deployment {
            deployment.policy_results
        } else {
            vec![]
        },
        initiated_by,
        payload.cpu.clone(),
        payload.memory.clone(),
        payload.reference.clone(),
    )
}

async fn check_dependencies<'a>(
    payload: &ApiInfraPayload,
    handler: &GenericCloudHandler,
    status_handler: &mut DeploymentStatusHandler<'a>,
) {
    let mut dependencies_not_finished: Vec<env_defs::Dependency> = Vec::new();
    for dep in &payload.dependencies {
        match check_dependency_status(dep).await {
            Ok(_) => {
                println!("Dependency finished");
            }
            Err(e) => {
                println!("Dependency not finished: {:?}", e);
                dependencies_not_finished.push(dep.clone());
            }
        }
    }

    if !dependencies_not_finished.is_empty() {
        let status = "waiting-on-dependency".to_string();
        // status_handler.set_error_text(error_text);
        status_handler.set_status(status);
        status_handler.set_event_duration();
        status_handler.send_event(handler).await;
        status_handler.send_deployment(handler).await;
        exit(0);
    }
}

async fn check_dependants<'a>(
    payload: &ApiInfraPayload,
    handler: &GenericCloudHandler,
    status_handler: &mut DeploymentStatusHandler<'a>,
) {
    let deployment_id = &payload.deployment_id;
    let environment = &payload.environment;

    let (_, dependants) = match handler
        .get_deployment_and_dependents(deployment_id, environment, false)
        .await
    {
        Ok(deployment_and_dependants) => deployment_and_dependants,
        Err(e) => {
            println!("Error getting deployment and dependants: {}", e);
            let status = "error".to_string();
            status_handler
                .set_error_text(format!("Error getting deployment and dependants: {}", e));
            status_handler.set_status(status);
            status_handler.set_event_duration();
            status_handler.send_event(handler).await;
            status_handler.send_deployment(handler).await;
            exit(1);
        }
    };

    if !dependants.is_empty() {
        let status = "has-dependants".to_string();
        status_handler.set_error_text("This deployment has other deployments depending on it, and hence cannot be removed until they are removed".to_string());
        status_handler.set_status(status);
        status_handler.set_event_duration();
        status_handler.send_event(handler).await;
        status_handler.send_deployment(handler).await;
        exit(0);
    }
}

async fn setup_misc() {
    if env::var("DEBUG_PRINT_ALL_ENV_VARS").is_ok() {
        for (key, value) in env::vars() {
            println!("{}: {}", key, value);
        }
    }

    if env::var("AZURE_CONTAINER_INSTANCE").is_ok() {
        // TODO: Move this?
        // Following is necessary since the oauth2 endpoint takes some time to be ready in Azure Container Instances
        println!("Running in Azure Container Instance, waiting for network to be ready...");
        std::thread::sleep(std::time::Duration::from_secs(25)); // TODO: Replace with a loop that checks if the endpoint is ready
        println!("Network should be ready now");
    };
}
