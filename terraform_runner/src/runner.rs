use anyhow::{anyhow, Result};
use env_common::interface::GenericCloudHandler;
use env_common::logic::{driftcheck_infra, publish_notification};
use env_common::DeploymentStatusHandler;
use env_defs::{
    ApiInfraPayload, ApiInfraPayloadWithVariables, CloudProvider, Dependency, Dependent,
    DeploymentResp, DeploymentStatus, ExtraData, JobDetails, NotificationData,
};
use env_utils::{store_backend_file, store_tf_vars_json};
use futures::future::join_all;
use futures::FutureExt;
use log::{error, info};
use serde_json::{json, Value};
use std::any::Any;
use std::env;
use std::panic::AssertUnwindSafe;
use std::process::exit;
use std::vec;

use crate::module::{download_module, get_module};
use crate::terraform::terraform_graph;
use crate::{
    get_initial_deployment, record_apply_destroy_changes, run_opa_policy_checks,
    set_up_provider_mirror, terraform_apply_destroy, terraform_init, terraform_output,
    terraform_plan, terraform_show, terraform_state_list, terraform_validate,
};

pub async fn run_terraform_runner(
    handler: &GenericCloudHandler,
) -> Result<(), Box<dyn std::error::Error>> {
    let payload = parse_payload_env_var();

    // Skeleton with empty variables; real values are fetched from the DB
    // inside the guarded section below and patched in via set_variables.
    let payload_with_variables = ApiInfraPayloadWithVariables {
        payload,
        variables: Value::Null,
    };
    let mut status_handler = initiate_deployment_status_handler(&None, &payload_with_variables);

    let flow_result = run_runner_flow_crash_guarded(
        handler,
        &mut status_handler,
        &payload_with_variables.payload,
    )
    .await;
    let completion = finish_runner_flow(handler, &mut status_handler, flow_result).await;

    publish_runner_notification(
        handler,
        &payload_with_variables.payload,
        &status_handler,
        &completion,
    )
    .await?;

    log::info!("Done!");

    Ok(())
}

struct RunnerCompletion {
    status: &'static str,
    error_text: String,
}

async fn run_runner_flow_crash_guarded<'a>(
    handler: &GenericCloudHandler,
    status_handler: &mut DeploymentStatusHandler<'a>,
    payload: &'a ApiInfraPayload,
) -> Result<(), anyhow::Error> {
    let outcome = AssertUnwindSafe(run_runner_inner(handler, status_handler, payload))
        .catch_unwind()
        .await;

    match outcome {
        Ok(result) => result,
        Err(panic) => Err(anyhow!(
            "Terraform runner panicked: {}",
            panic_message(panic.as_ref())
        )),
    }
}

fn panic_message(panic: &(dyn Any + Send)) -> String {
    if let Some(s) = panic.downcast_ref::<&'static str>() {
        (*s).to_string()
    } else if let Some(s) = panic.downcast_ref::<String>() {
        s.clone()
    } else {
        "unknown panic".to_string()
    }
}

async fn finish_runner_flow(
    handler: &GenericCloudHandler,
    status_handler: &mut DeploymentStatusHandler<'_>,
    flow_result: Result<(), anyhow::Error>,
) -> RunnerCompletion {
    match flow_result {
        Ok(_) => {
            info!("Terraform flow completed successfully");
            RunnerCompletion {
                status: "success",
                error_text: String::new(),
            }
        }
        Err(e) => {
            error!("Terraform runner failed: {:?}", e);
            let error_text = e.to_string();
            flush_failed_status_if_needed(handler, status_handler, &error_text).await;
            RunnerCompletion {
                status: "failure",
                error_text,
            }
        }
    }
}

async fn flush_failed_status_if_needed(
    handler: &GenericCloudHandler,
    status_handler: &mut DeploymentStatusHandler<'_>,
    error_text: &str,
) {
    if status_handler.get_status().is_final() {
        return;
    }

    // Safety net: sub-steps normally update the deployment status before
    // returning, but if one short-circuits or panics before doing so, this
    // keeps callers from seeing the job stuck in a non-terminal state.
    status_handler.set_status(DeploymentStatus::Failed);
    status_handler.set_error_text(error_text.to_string());
    status_handler.set_event_duration();
    status_handler.send_event(handler).await;

    if let Err(send_err) = status_handler.send_deployment(handler).await {
        error!(
            "Failed to write final deployment failure status: {:?}",
            send_err
        );
    }
}

async fn publish_runner_notification(
    handler: &GenericCloudHandler,
    payload: &ApiInfraPayload,
    status_handler: &DeploymentStatusHandler<'_>,
    completion: &RunnerCompletion,
) -> Result<(), anyhow::Error> {
    let job_id = status_handler.get_job_id().to_string();
    let mut extra_data = payload.extra_data.clone();

    match extra_data {
        ExtraData::GitHub(ref mut github_data) => {
            github_data.job_details = JobDetails {
                region: payload.region.clone(),
                environment: payload.environment.clone(),
                deployment_id: payload.deployment_id.clone(),
                job_id: job_id.clone(),
                change_type: payload.command.to_uppercase(),
                file_path: github_data.job_details.file_path.clone(),
                error_text: completion.error_text.clone(),
                status: completion.status.to_string(),
            };
        }
        ExtraData::GitLab(ref mut gitlab_data) => {
            gitlab_data.job_details = JobDetails {
                region: payload.region.clone(),
                environment: payload.environment.clone(),
                deployment_id: payload.deployment_id.clone(),
                job_id: job_id.clone(),
                change_type: payload.command.to_uppercase(),
                file_path: gitlab_data.job_details.file_path.clone(),
                error_text: completion.error_text.clone(),
                status: completion.status.to_string(),
            };
        }
        ExtraData::None => {}
    }

    let notification = NotificationData {
        subject: "runner_event".to_string(),
        message: serde_json::to_value(extra_data)?,
    };
    publish_notification(handler, notification).await?;
    Ok(())
}

async fn run_runner_inner<'a>(
    handler: &GenericCloudHandler,
    status_handler: &mut DeploymentStatusHandler<'a>,
    payload: &'a ApiInfraPayload,
) -> Result<(), anyhow::Error> {
    // Due to length constraints in environment variables, deployment claim
    // variables are fetched from the database here.
    let (variables, job_id_for_variables) = fetch_deployment_variables(handler, payload).await?;
    status_handler.set_variables(variables.clone());

    log::info!("Storing terraform variables in tf_vars.json...");
    store_tf_vars_json(&variables, ".");
    store_backend_file(
        GenericCloudHandler::default().await.get_backend_provider(),
        ".",
        &json!({}),
    )
    .await;

    log::info!("Read deployment id from environment variable...");

    let command = &payload.command;
    let refresh_only = payload.flags.iter().any(|e| e == "-refresh-only");

    let initial_deployment = get_initial_deployment(payload, handler).await?;
    if let Some(d) = &initial_deployment {
        status_handler.set_output(d.output.clone());
        status_handler.set_policy_results(d.policy_results.clone());
    }

    let job_id = get_current_job_id(handler, status_handler).await?;
    ensure_valid_job_id(status_handler, handler, &job_id, &job_id_for_variables).await?;

    // Mark that the deployment has started
    if command == "plan" && refresh_only {
        status_handler.set_is_drift_check();
    }
    status_handler.send_event(handler).await;
    status_handler.send_deployment(handler).await?;

    terraform_flow(handler, status_handler, payload, &job_id).await
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
            log::info!("Pre-downloaded all providers from storage");
        }
        Err(e) => {
            log::info!(
                "An error occurred while pre-downloading terraform providers: {:?}, continuing...",
                e
            );
        }
    }

    download_module(handler, &module, status_handler).await?;

    terraform_init(payload, handler, status_handler).await?;

    terraform_validate(payload, handler, status_handler).await?;

    let plan_std_output = terraform_plan(payload, handler, status_handler).await?;

    terraform_show(
        payload,
        job_id,
        &module,
        &plan_std_output,
        handler,
        status_handler,
        true,
    )
    .await?;

    terraform_graph(payload, job_id, handler, status_handler).await?;

    run_opa_policy_checks(handler, status_handler).await?;

    if command == "apply" || command == "destroy" {
        let apply_result = terraform_apply_destroy(payload, handler, status_handler).await;

        terraform_show(
            payload,
            job_id,
            &module,
            &plan_std_output,
            handler,
            status_handler,
            false,
        )
        .await?;

        // Always capture the current state resources, even if apply/destroy failed partway through
        // This ensures we have an accurate record of what resources actually exist
        let captured_resources = match terraform_state_list().await {
            Ok(tf_resources) => {
                status_handler.set_resources(tf_resources.clone());
                tf_resources
            }
            Err(e) => {
                log::warn!("Failed to capture resource list: {:?}", e);
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
                log::info!("Successfully recorded apply/destroy changes");
            }
            Err(e) => {
                log::warn!("Failed to record apply/destroy changes: {:?}", e);
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
                log::info!(
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
    status_handler.set_status(DeploymentStatus::Successful);
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
            log::info!(
                "Deploymentid: {}, environment: {}",
                deployment_id,
                environment
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

/// Parse the PAYLOAD env var into an ApiInfraPayload. A parse failure leaves
/// us with no deployment_id, so there is no row to update - this is the only
/// failure path that intentionally exits the process.
fn parse_payload_env_var() -> ApiInfraPayload {
    let payload_env = env::var("PAYLOAD").unwrap();
    match serde_json::from_str(&payload_env) {
        Ok(json) => json,
        Err(e) => {
            log::error!(
                "Failed to parse env-var PAYLOAD as ApiInfraPayload: {:?}",
                e
            );
            exit(1);
        }
    }
}

/// Fetch the variables and authoritative job_id for the current deployment.
/// Returns Err on any failure so the caller can flush a terminal deployment
/// status before bailing.
async fn fetch_deployment_variables(
    handler: &GenericCloudHandler,
    payload: &ApiInfraPayload,
) -> Result<(Value, String), anyhow::Error> {
    let result = match payload.command.as_str() {
        "plan" => {
            let job_id = handler
                .get_current_job_id()
                .await
                .map_err(|e| anyhow!("Error getting current job id for plan: {}", e))?;
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

    match result {
        Ok(Some(deployment)) => Ok((deployment.variables, deployment.job_id)),
        Ok(None) => Err(anyhow!(
            "Deployment not found: {} in {}",
            payload.deployment_id,
            payload.environment
        )),
        Err(e) => Err(anyhow!("Error getting deployment: {}", e)),
    }
}

async fn ensure_valid_job_id(
    status_handler: &mut DeploymentStatusHandler<'_>,
    handler: &GenericCloudHandler,
    job_id: &str,
    job_id_for_variables: &str,
) -> Result<(), anyhow::Error> {
    // This is a safeguard to ensure that the job_id fetched from the environment variable matches the one in the database.
    // Will always be true for plan command, but is important for apply and destroy commands to ensure that the variables match.
    if job_id != job_id_for_variables {
        let error_text = format!(
            "Job ID does not match the one in the database, which means that the variables cannot be trusted: {} != {}",
            job_id, job_id_for_variables
        );
        log::info!("{}", &error_text);
        status_handler.set_error_text(error_text.clone());
        status_handler.set_status(DeploymentStatus::Failed);
        status_handler.set_event_duration();
        status_handler.send_event(handler).await;
        let _ = status_handler.send_deployment(handler).await;
        return Err(anyhow!(error_text));
    }
    Ok(())
}

// fn cat_file(filename: &str) {
//     log::info!("=== File content: {} ===", filename);
//     let output = std::process::Command::new("cat")
//         .arg(filename)
//         .output()
//         .expect("Failed to execute command");

//     log::info!("{}", String::from_utf8_lossy(&output.stdout));
// }

async fn check_dependency_status(dependency: &Dependency) -> Result<(), anyhow::Error> {
    log::info!("Checking dependency status...");
    let handler = GenericCloudHandler::default().await;
    match handler
        .get_deployment(&dependency.deployment_id, &dependency.environment, false)
        .await
    {
        Ok(deployment) => match deployment {
            Some(deployment) => {
                if deployment.status == DeploymentStatus::Successful {
                    Ok(())
                } else {
                    Err(anyhow!("Dependency not finished"))
                }
            }
            None => panic!("Deployment could not describe since it was not found"),
        },
        Err(e) => {
            log::info!("Error: {:?}", e);
            panic!("Error getting deployment status");
        }
    }
}

async fn get_current_job_id(
    handler: &GenericCloudHandler,
    status_handler: &mut DeploymentStatusHandler<'_>,
) -> Result<String, anyhow::Error> {
    match handler.get_current_job_id().await {
        Ok(id) => {
            status_handler.set_job_id(id.clone());
            Ok(id)
        }
        Err(e) => {
            log::info!("Error getting current job id: {:?}", e);
            status_handler.set_error_text(
                "The job failed to fetch the job id, please retry again.".to_string(),
            );
            status_handler.set_status(DeploymentStatus::Failed);
            status_handler.set_event_duration();
            status_handler.send_event(handler).await;
            let _ = status_handler.send_deployment(handler).await;
            Err(anyhow!("Failed to fetch current job id: {}", e))
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
    let status = DeploymentStatus::Initiated;
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
                log::info!("Dependency finished");
            }
            Err(e) => {
                log::debug!("Dependency not finished: {:?}", e);
                dependencies_not_finished.push(dep.clone());
            }
        }
    }

    if !dependencies_not_finished.is_empty() {
        let status = DeploymentStatus::WaitingOnDependency;
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
            log::info!("Error getting deployment and dependants: {}", e);
            let status = DeploymentStatus::Error;
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
        let status = DeploymentStatus::HasDependants;
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
            log::info!("{}: {}", key, value);
        }
    }

    if env::var("AZURE_CONTAINER_INSTANCE").is_ok() {
        // TODO: Move this?
        // Following is necessary since the oauth2 endpoint takes some time to be ready in Azure Container Instances
        log::info!("Running in Azure Container Instance, waiting for network to be ready...");
        std::thread::sleep(std::time::Duration::from_secs(25)); // TODO: Replace with a loop that checks if the endpoint is ready
        log::info!("Network should be ready now");
    };
}
