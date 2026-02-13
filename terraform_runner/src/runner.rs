use anyhow::{anyhow, Result};
use env_common::interface::GenericCloudHandler;
use env_common::logic::{driftcheck_infra, publish_notification};
use env_common::DeploymentStatusHandler;
use env_defs::{
    ApiInfraPayload, ApiInfraPayloadWithVariables, CloudProvider, Dependency, Dependent,
    DeploymentResp, ExtraData, JobDetails, NotificationData,
};
use env_utils::{store_backend_file, store_tf_vars_json};
use futures::future::join_all;
use log::{error, info};
use serde_json::{json, Value};
use std::env;
use std::process::exit;
use std::vec;

use crate::module::{download_module, get_module};
use crate::{
    get_initial_deployment, record_apply_destroy_changes, run_opa_policy_checks,
    set_up_provider_mirror, terraform_apply_destroy, terraform_init, terraform_output,
    terraform_plan, terraform_show, terraform_state_list, terraform_validate,
};

pub async fn run_terraform_runner(
    handler: &GenericCloudHandler,
) -> Result<(), Box<dyn std::error::Error>> {
    // Due to length constraints in environment variables, deployment claim variables need to be fetched from the database
    let (payload_with_variables, job_id_for_variables) = get_payload_with_variables(handler).await;
    let payload = &payload_with_variables.payload;

    println!("Storing terraform variables in tf_vars.json...");
    store_tf_vars_json(&payload_with_variables.variables, ".");
    store_backend_file(
        GenericCloudHandler::default().await.get_backend_provider(),
        ".",
        &json!({}),
    )
    .await;

    println!("Read deployment id from environment variable...");

    let command = &payload.command;
    let refresh_only = payload.flags.iter().any(|e| e == "-refresh-only");

    let initial_deployment = get_initial_deployment(payload, handler).await;

    // To reduce clutter, a DeploymentStatusHandler is used to handle the status updates
    // since we will be updating the status multiple times and only a few fields change each time
    let mut status_handler =
        initiate_deployment_status_handler(&initial_deployment, &payload_with_variables);
    let job_id = get_current_job_id(handler, &mut status_handler).await;

    ensure_valid_job_id(
        // TODO: handle error better, this is just a safe guard
        &mut status_handler,
        handler,
        &job_id,
        &job_id_for_variables,
    )
    .await;

    // Mark that the deployment has started
    if command == "plan" && refresh_only {
        status_handler.set_is_drift_check();
    }
    status_handler.send_event(handler).await;
    status_handler.send_deployment(handler).await?;

    let (result, error_text) =
        match terraform_flow(handler, &mut status_handler, payload, &job_id).await {
            Ok(_) => {
                info!("Terraform flow completed successfully");
                ("success", "".to_string())
            }
            Err(e) => {
                error!("Terraform flow failed: {:?}", e);
                ("failure", e.to_string())
            }
        };

    let mut extra_data = payload.extra_data.clone();
    match extra_data {
        ExtraData::GitHub(ref mut github_data) => {
            github_data.job_details = JobDetails {
                region: payload.region.clone(),
                environment: payload.environment.clone(),
                deployment_id: payload.deployment_id.clone(),
                job_id: job_id.clone(),
                change_type: command.to_uppercase(),
                file_path: github_data.job_details.file_path.clone(),
                error_text,
                status: result.to_string(),
            };
        }
        ExtraData::GitLab(ref mut gitlab_data) => {
            gitlab_data.job_details = JobDetails {
                region: payload.region.clone(),
                environment: payload.environment.clone(),
                deployment_id: payload.deployment_id.clone(),
                job_id: job_id.clone(),
                change_type: command.to_uppercase(),
                file_path: gitlab_data.job_details.file_path.clone(),
                error_text,
                status: result.to_string(),
            };
        }
        ExtraData::None => {}
    }

    let notification = NotificationData {
        subject: "runner_event".to_string(),
        message: serde_json::to_value(extra_data)?,
    };
    publish_notification(handler, notification).await.unwrap();

    println!("Done!");

    Ok(())
}

async fn terraform_flow<'a>(
    handler: &GenericCloudHandler,
    status_handler: &mut DeploymentStatusHandler<'a>,
    payload: &'a ApiInfraPayload,
    job_id: &str,
) -> Result<(), anyhow::Error> {
    let command = &payload.command;

    // Check if there are any dependencies that are not finished
    if command == "apply" {
        // Check if all dependencies have state = successful, if not, store "waiting-on-dependency" status and exit
        check_dependencies(payload, handler, status_handler).await?;
    } else if command == "destroy" {
        // Check if there are any deployments that is depending on this deployment, if so, store "has-dependants" status and exit
        check_dependants(payload, handler, status_handler).await?;
    }

    let module = get_module(handler, payload, status_handler).await?;

    match set_up_provider_mirror(handler, &module.tf_lock_providers, "linux_arm64").await {
        Ok(_) => {
            println!("Pre-downloaded all providers from storage");
        }
        Err(e) => {
            println!(
                "An error occurred while pre-downloading terraform providers: {:?}, continuing...",
                e
            );
        }
    }

    download_module(handler, &module, status_handler).await?;

    terraform_init(payload, handler, status_handler).await?;

    terraform_validate(payload, handler, status_handler).await?;

    let plan_output = terraform_plan(payload, handler, status_handler).await?;

    terraform_show(
        payload,
        job_id,
        &module,
        &plan_output,
        handler,
        status_handler,
    )
    .await?;

    run_opa_policy_checks(handler, status_handler).await?;

    if command == "apply" || command == "destroy" {
        let apply_result = terraform_apply_destroy(payload, handler, status_handler).await;

        // Always capture the current state resources, even if apply/destroy failed partway through
        // This ensures we have an accurate record of what resources actually exist
        let captured_resources = match terraform_state_list().await {
            Ok(tf_resources) => {
                status_handler.set_resources(tf_resources.clone());
                tf_resources
            }
            Err(e) => {
                println!("Warning: Failed to capture resource list: {:?}", e);
                None
            }
        };

        // Extract output for subsequent operations
        let apply_output_str = apply_result.as_ref().map(|s| s.as_str()).unwrap_or("");

        // Record the apply/destroy operation in the change history
        match record_apply_destroy_changes(
            payload,
            job_id,
            &module,
            apply_output_str,
            handler,
            status_handler,
        )
        .await
        {
            Ok(_) => {
                println!("Successfully recorded apply/destroy changes");
            }
            Err(e) => {
                println!("Warning: Failed to record apply/destroy changes: {:?}", e);
            }
        }

        // Handle apply/destroy errors after capturing resources and change records
        if let Err(e) = apply_result {
            let is_destroy = command == "destroy";
            let has_no_resources = captured_resources
                .as_ref()
                .map(|r| r.is_empty())
                .unwrap_or(false);

            // Allow destroy to proceed if there are no resources in state
            // This prevents users from getting stuck when trying to clean up deployments
            // that have no actual infrastructure resources
            if is_destroy && has_no_resources {
                println!(
                    "Destroy failed but no resources exist in state - proceeding with cleanup: {:?}",
                    e
                );
                status_handler.set_deleted(true);
            } else {
                // Re-propagate error for apply failures or destroy with existing resources
                return Err(e);
            }
        }

        // Only get outputs for apply command (destroy has no outputs since resources are gone)
        if command == "apply" {
            terraform_output(payload, handler, status_handler).await?;
        }
    }

    // Set deployment status to successful after all operations complete
    status_handler.set_status("successful".to_string());
    status_handler.set_event_duration();
    status_handler.set_last_event_epoch();
    status_handler.send_event(handler).await;
    status_handler.send_deployment(handler).await?;

    // if !dependents.is_empty() {
    //     _trigger_dependent_deployments(&dependents).await; // TODO: WIP: needs to launch with replaced variables
    // }

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
            match driftcheck_infra(
                &handler,
                &deployment_id,
                &environment,
                remediate,
                ExtraData::None,
            )
            .await
            {
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

async fn get_payload_with_variables(
    handler: &GenericCloudHandler,
) -> (ApiInfraPayloadWithVariables, String) {
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

    let result = match &payload.command.as_str() {
        &"plan" => {
            let job_id = handler.get_current_job_id().await.unwrap();
            handler
                .get_plan_deployment(&payload.deployment_id, &payload.environment, &job_id)
                .await
        }
        _ => {
            // For other commands, fetch the deployment as usual (apply, destroy)
            handler
                .get_deployment(&payload.deployment_id, &payload.environment, false)
                .await
        }
    };

    let (variables, job_id) = match result {
        Ok(deployment) => match deployment {
            Some(deployment) => (deployment.variables, deployment.job_id),
            None => {
                eprintln!(
                    "Deployment not found: {} in {}",
                    payload.deployment_id, payload.environment
                );
                std::process::exit(1);
            }
        },
        Err(e) => {
            eprintln!("Error getting deployment: {:?}", e);
            std::process::exit(1);
        }
    };

    (ApiInfraPayloadWithVariables { payload, variables }, job_id)
}

async fn ensure_valid_job_id(
    status_handler: &mut DeploymentStatusHandler<'_>,
    handler: &GenericCloudHandler,
    job_id: &str,
    job_id_for_variables: &str,
) {
    // This is a safeguard to ensure that the job_id fetched from the environment variable matches the one in the database.
    // Will always be true for plan command, but is important for apply and destroy commands to ensure that the variables match.
    if job_id != job_id_for_variables {
        let error_text = format!("Job ID does not match the one in the database, which means that the variables cannot be trusted: {} != {}", job_id, job_id_for_variables);
        println!("{}", &error_text);
        let status = "failed".to_string();
        status_handler.set_error_text(error_text);
        status_handler.set_status(status);
        status_handler.set_event_duration();
        status_handler.send_event(handler).await;
        let _ = status_handler.send_deployment(handler).await;
        exit(1);
    }
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
            let _ = status_handler.send_deployment(handler).await;
            exit(1);
        }
    }
}

fn initiate_deployment_status_handler<'a>(
    initial_deployment: &Option<DeploymentResp>,
    payload_with_variables: &'a ApiInfraPayloadWithVariables,
) -> DeploymentStatusHandler<'a> {
    let payload = &payload_with_variables.payload;
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
        payload_with_variables.variables.clone(),
        payload.drift_detection.clone(),
        payload.next_drift_check_epoch,
        payload.dependencies.clone(),
        if initial_deployment.is_some() {
            initial_deployment.clone().unwrap().output
        } else {
            Value::Null
        },
        if let Some(deployment) = initial_deployment {
            deployment.policy_results.clone()
        } else {
            vec![]
        },
        initiated_by,
        payload.cpu.clone(),
        payload.memory.clone(),
        payload.reference.clone(),
    )
}

async fn check_dependencies(
    payload: &ApiInfraPayload,
    handler: &GenericCloudHandler,
    status_handler: &mut DeploymentStatusHandler<'_>,
) -> Result<(), anyhow::Error> {
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
        status_handler.send_deployment(handler).await?;
        return Err(anyhow!("Dependencies not finished"));
    }

    Ok(())
}

async fn check_dependants(
    payload: &ApiInfraPayload,
    handler: &GenericCloudHandler,
    status_handler: &mut DeploymentStatusHandler<'_>,
) -> Result<(), anyhow::Error> {
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
            status_handler.send_deployment(handler).await?;
            return Err(anyhow!("Error getting deployment and dependants"));
        }
    };

    if !dependants.is_empty() {
        let status = "has-dependants".to_string();
        status_handler.set_error_text("This deployment has other deployments depending on it, and hence cannot be removed until they are removed".to_string());
        status_handler.set_status(status);
        status_handler.set_event_duration();
        status_handler.send_event(handler).await;
        status_handler.send_deployment(handler).await?;
        return Err(anyhow!("This deployment has dependants"));
    }

    Ok(())
}

pub async fn setup_misc() {
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
