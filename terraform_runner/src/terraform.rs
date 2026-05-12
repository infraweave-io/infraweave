use env_common::logic::insert_infra_change_record;
use env_common::DeploymentStatusHandler;
use env_common::{interface::GenericCloudHandler, logic::upload_file_to_change_records};
use env_defs::{
    sanitize_resource_changes_from_plan, ApiInfraPayload, CloudProvider, DeploymentStatus,
    InfraChangeRecord, TfLockProvider,
};
use env_utils::{get_epoch, get_extra_environment_variables, get_provider_url_key, get_timestamp};
use futures::stream::{self, StreamExt};
use std::{
    env,
    path::{Path, PathBuf},
};
use tokio::fs;

use serde_json::Value;

use anyhow::{anyhow, Context, Result};

use crate::{post_webhook, run_generic_command, CommandResult};

#[allow(clippy::too_many_arguments)]
#[tracing::instrument(skip_all, fields(command = %command))]
pub async fn run_terraform_command(
    command: &str,
    refresh_only: bool,
    no_lock_flag: bool,
    destroy_flag: bool,
    auto_approve_flag: bool,
    no_input_flag: bool,
    json_flag: bool,
    plan_out: bool,
    plan_in: bool,
    init: bool,
    deployment_id: &str,
    environment: &str,
    max_output_lines: usize,
    extra_environment_variables: Option<&std::collections::HashMap<String, String>>,
) -> Result<CommandResult, anyhow::Error> {
    let mut exec = tokio::process::Command::new("terraform");
    exec.arg(command)
        .arg("-no-color")
        .current_dir(Path::new("./"))
        .env("TF_CLI_CONFIG_FILE", terraform_cli_config_file())
        .env("TF_PLUGIN_CACHE_DIR", terraform_plugin_cache_dir())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped()); // Capture stdout

    if let Some(env_vars) = extra_environment_variables {
        for (key, value) in env_vars {
            exec.env(format!("TF_VAR_{}", key), value);
        }
    }

    if refresh_only {
        exec.arg("-refresh-only");
    }

    if no_input_flag {
        exec.arg("-input=false");
    }

    if auto_approve_flag {
        exec.arg("-auto-approve");
    }

    if destroy_flag {
        exec.arg("-destroy");
    }

    if json_flag {
        exec.arg("-json");
    }

    if plan_in {
        exec.arg("planfile");
    }

    if plan_out {
        exec.arg("-out=planfile");
    }

    if no_lock_flag {
        // Allow multiple plans to be run in parallel, without locking the state
        exec.arg("-lock=false");
    }

    log::info!("Running terraform command: terraform {}", command);

    if init {
        GenericCloudHandler::default()
            .await
            .set_backend(&mut exec, deployment_id, environment)
            .await;
    }

    // Don't print output for graph, show, or state commands to avoid cluttering
    let print_output = !["graph", "show", "state"].contains(&command);
    run_generic_command(&mut exec, max_output_lines, print_output).await
}

#[tracing::instrument(skip_all, fields(cmd = %payload.command, module = %payload.module, version = %payload.module_version))]
pub async fn terraform_init(
    payload: &ApiInfraPayload,
    handler: &GenericCloudHandler,
    status_handler: &mut DeploymentStatusHandler<'_>,
) -> Result<(), anyhow::Error> {
    let deployment_id = &payload.deployment_id;
    let environment = &payload.environment;

    let cmd = "init";
    match run_terraform_command(
        cmd,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        true,
        deployment_id,
        environment,
        50,
        None,
    )
    .await
    {
        Ok(_) => {
            log::info!("Terraform init successful");
            Ok(())
        }
        Err(e) => {
            log::info!("Error running \"terraform {}\" command: {:?}", cmd, e);
            let status = DeploymentStatus::FailedInit;
            status_handler.set_status(status);
            status_handler.set_event_duration();
            status_handler.send_event(handler).await;
            status_handler.send_deployment(handler).await?;
            Err(anyhow!("Error running terraform init: {}", e))
        }
    }
}

#[tracing::instrument(skip_all, fields(cmd = %payload.command, module = %payload.module, version = %payload.module_version))]
pub async fn terraform_validate(
    payload: &ApiInfraPayload,
    handler: &GenericCloudHandler,
    status_handler: &mut DeploymentStatusHandler<'_>,
) -> Result<(), anyhow::Error> {
    let deployment_id = &payload.deployment_id;
    let environment = &payload.environment;

    let cmd = "validate";
    match run_terraform_command(
        cmd,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        deployment_id,
        environment,
        50,
        None,
    )
    .await
    {
        Ok(_) => {
            log::info!("Terraform {} successful", cmd);
            Ok(())
        }
        Err(e) => {
            log::info!("Error running \"terraform {}\" command: {:?}", cmd, e);
            let error_text: String = e.to_string();
            let status = DeploymentStatus::FailedValidate;
            status_handler.set_status(status);
            status_handler.set_event_duration();
            status_handler.set_error_text(error_text);
            status_handler.send_event(handler).await;
            status_handler.send_deployment(handler).await?;
            status_handler.set_error_text("".to_string());
            Err(anyhow!("Error running terraform validate: {}", e))
        }
    }
}

#[tracing::instrument(skip_all, fields(cmd = "graph", module = %payload.module, version = %payload.module_version))]
pub async fn terraform_graph(
    payload: &ApiInfraPayload,
    job_id: &str,
    handler: &GenericCloudHandler,
    status_handler: &mut DeploymentStatusHandler<'_>,
) -> Result<(), anyhow::Error> {
    let deployment_id = &payload.deployment_id;
    let environment = &payload.environment;

    let cmd = "graph";
    match run_terraform_command(
        cmd,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        deployment_id,
        environment,
        usize::MAX,
        None,
    )
    .await
    {
        Ok(command_result) => {
            log::info!("Terraform graph successful");

            let graph_dot = "./graph.dot";
            let graph_dot_file_path = Path::new(graph_dot);
            let graph_json = &command_result.stdout;
            // Write the stdout content to the file without parsing to be uploaded later
            std::fs::write(graph_dot_file_path, graph_json).expect("Unable to write to file");

            let graph_raw_json_key = format!(
                "{}{}/{}/{}_graph.dot",
                handler.get_storage_basepath(),
                environment,
                deployment_id,
                job_id
            );

            match upload_file_to_change_records(handler, &graph_raw_json_key, graph_json).await {
                Ok(_) => {
                    log::info!("Successfully uploaded graph output file");
                }
                Err(e) => {
                    log::error!("Failed to upload graph output file: {}", e);
                }
            }

            Ok(())
        }
        Err(e) => {
            log::info!("Error running \"terraform {}\" command: {:?}", cmd, e);
            let error_text: String = e.to_string();
            let status = DeploymentStatus::FailedGraph;
            status_handler.set_status(status);
            status_handler.set_event_duration();
            status_handler.set_error_text(error_text);
            status_handler.send_event(handler).await;
            status_handler.send_deployment(handler).await?;
            status_handler.set_error_text("".to_string());
            Err(anyhow!(
                "Error running \"terraform {}\" command: {}",
                cmd,
                e
            ))
        }
    }
}

#[tracing::instrument(skip_all, fields(cmd = %payload.command, module = %payload.module, version = %payload.module_version))]
pub async fn terraform_plan(
    payload: &ApiInfraPayload,
    handler: &GenericCloudHandler,
    status_handler: &mut DeploymentStatusHandler<'_>,
) -> Result<String, anyhow::Error> {
    let deployment_id = &payload.deployment_id;
    let environment = &payload.environment;

    let command = &payload.command;
    let refresh_only = payload.flags.iter().any(|e| e == "-refresh-only");
    let destroy_flag = payload.flags.iter().any(|e| e == "-destroy");
    let no_lock_flag = payload.flags.iter().any(|e| e == "-no-lock");

    match run_terraform_command(
        "plan",
        refresh_only,
        no_lock_flag || command == "plan",
        destroy_flag || command == "destroy",
        false,
        true,
        false,
        true,
        false,
        false,
        deployment_id,
        environment,
        500,
        Some(&get_extra_environment_variables(payload)),
    )
    .await
    {
        Ok(command_result) => {
            log::info!("Terraform plan successful");
            Ok(command_result.stdout)
        }
        Err(e) => {
            log::info!("Error running \"terraform plan\" command: {:?}", e);
            let error_text = e.to_string();
            let status = DeploymentStatus::FailedPlan;
            status_handler.set_status(status);
            status_handler.set_event_duration();
            status_handler.set_error_text(error_text);
            status_handler.send_event(handler).await;
            status_handler.send_deployment(handler).await?;
            status_handler.set_error_text("".to_string());
            Err(anyhow!("Error running terraform plan: {}", e))
        }
    }
}

#[tracing::instrument(skip_all, fields(cmd = %payload.command, job_id = %job_id, module = %module.module, version = %module.version))]
pub async fn terraform_show(
    payload: &ApiInfraPayload,
    job_id: &str,
    module: &env_defs::ModuleResp,
    plan_std_output: &str,
    handler: &GenericCloudHandler,
    status_handler: &mut DeploymentStatusHandler<'_>,
    use_planfile: bool,
) -> Result<(), anyhow::Error> {
    let deployment_id = &payload.deployment_id;
    let environment = &payload.environment;
    let project_id = &payload.project_id;
    let region = &payload.region;

    let command = &payload.command;
    let refresh_only = payload.flags.iter().any(|e| e == "-refresh-only");

    let cmd = "show";
    match run_terraform_command(
        cmd,
        false,
        false,
        false,
        false,
        false,
        true,
        false,
        use_planfile,
        false,
        deployment_id,
        environment,
        5000,
        None,
    )
    .await
    {
        Ok(command_result) => {
            log::info!("Terraform {} successful", cmd);

            let (output_filename, upload_suffix) = if use_planfile {
                ("tf_plan.json", "plan_output.json")
            } else {
                ("tf_state.json", "state_output.json")
            };

            let tf_output_file_path = Path::new(output_filename);

            let output_json = &command_result.stdout;

            // Write the stdout content to the file without parsing to be used for OPA policy checks
            std::fs::write(tf_output_file_path, output_json).expect("Unable to write to file");

            let content: Value = serde_json::from_str(output_json).unwrap();

            let output_json_key = format!(
                "{}{}/{}/{}_{}",
                handler.get_storage_basepath(),
                environment,
                deployment_id,
                job_id,
                upload_suffix
            );

            match upload_file_to_change_records(handler, &output_json_key, output_json).await {
                Ok(_) => {
                    log::info!(
                        "Successfully uploaded \"tofu {cmd}\" output file ({})",
                        output_filename
                    );
                }
                Err(e) => {
                    log::error!("Failed to upload \"tofu {cmd}\" output file: {}", e);
                }
            }

            if command == "plan" && refresh_only {
                let drift_has_occurred = !content
                    .get("resource_drift")
                    .unwrap_or(&serde_json::from_str("[]").unwrap())
                    .as_array()
                    .unwrap()
                    .is_empty();
                status_handler.set_drift_has_occurred(drift_has_occurred);

                if drift_has_occurred {
                    for webhook in &payload.drift_detection.webhooks {
                        match &webhook.url {
                            Some(url) => {
                                match post_webhook(
                                    url,
                                    &format!(
                                        "Drift has occurred for {} in {}",
                                        deployment_id, environment
                                    ),
                                )
                                .await
                                {
                                    Ok(_) => {
                                        log::info!("Webhook {:?} sent successfully", webhook);
                                    }
                                    Err(e) => {
                                        log::info!(
                                            "Error sending webhook: {:?} with url: {:?}",
                                            e,
                                            webhook
                                        );
                                        // Don't fail the deployment if the webhook fails
                                    }
                                }
                            }
                            None => {
                                log::warn!("Webhook URL not provided");
                            }
                        }
                    }
                }
            }

            // Create InfraChangeRecord for the plan phase (use_planfile=true covers both standalone
            // plan commands and the plan phase of apply/destroy). For apply/destroy, an additional
            // MUTATE record is created after the operation in record_apply_destroy_changes.
            if use_planfile {
                let resource_changes = sanitize_resource_changes_from_plan(&content, refresh_only);

                let infra_change_record = InfraChangeRecord {
                    deployment_id: deployment_id.to_string(),
                    project_id: project_id.clone(),
                    region: region.to_string(),
                    job_id: job_id.to_string(),
                    module: module.module.clone(),
                    module_version: module.version.clone(),
                    epoch: get_epoch(),
                    timestamp: get_timestamp(),
                    plan_std_output: plan_std_output.to_string(),
                    plan_raw_json_key: output_json_key.clone(),
                    environment: environment.clone(),
                    change_type: "plan".to_string(),
                    resource_changes,
                    variables: status_handler.get_variables(),
                };
                match insert_infra_change_record(handler, infra_change_record).await {
                    Ok(_) => {
                        log::info!("Infra change record for plan inserted");
                    }
                    Err(e) => {
                        log::info!("Error inserting infra change record: {:?}", e);
                    }
                }
            }

            Ok(())
        }
        Err(e) => {
            log::info!("Error running \"terraform {}\" command: {:?}", cmd, e);
            let error_text = e.to_string();
            let status = DeploymentStatus::FailedShowPlan;
            status_handler.set_status(status);
            status_handler.set_event_duration();
            status_handler.set_error_text(error_text);
            status_handler.send_event(handler).await;
            status_handler.send_deployment(handler).await?;
            status_handler.set_error_text("".to_string());
            Err(anyhow!("Error running terraform show: {}", e))
        }
    }
}

#[tracing::instrument(skip_all, fields(cmd = %payload.command, job_id = %job_id, module = %module.module, version = %module.version))]
pub async fn record_apply_destroy_changes(
    payload: &ApiInfraPayload,
    job_id: &str,
    module: &env_defs::ModuleResp,
    apply_output: &str,
    handler: &GenericCloudHandler,
    status_handler: &DeploymentStatusHandler<'_>,
) -> Result<(), anyhow::Error> {
    // Extract resource changes from the plan JSON (what was approved before execution)
    let (resource_changes, raw_plan_json) = match tokio::fs::read_to_string("./tf_plan.json").await
    {
        Ok(plan_content) => match serde_json::from_str::<Value>(&plan_content) {
            Ok(content) => {
                let sanitized = sanitize_resource_changes_from_plan(&content, false);
                (sanitized, plan_content)
            }
            Err(_) => {
                log::warn!("Could not parse tf_plan.json, storing empty resource_changes");
                (Vec::new(), String::new())
            }
        },
        Err(_) => {
            log::warn!("Could not read tf_plan.json, storing empty resource_changes");
            (Vec::new(), String::new())
        }
    };

    let mutate_raw_json_key = format!(
        "{}{}/{}/{}_mutate_output.json",
        handler.get_storage_basepath(),
        payload.environment,
        payload.deployment_id,
        job_id,
    );

    match upload_file_to_change_records(handler, &mutate_raw_json_key, &raw_plan_json).await {
        Ok(_) => {
            log::info!("Successfully uploaded apply/destroy output file");
        }
        Err(e) => {
            log::error!("Failed to upload apply/destroy output file: {}", e);
        }
    }

    let infra_change_record = InfraChangeRecord {
        deployment_id: payload.deployment_id.clone(),
        project_id: payload.project_id.clone(),
        region: payload.region.clone(),
        job_id: job_id.to_string(),
        module: module.module.clone(),
        module_version: module.version.clone(),
        epoch: get_epoch(),
        timestamp: get_timestamp(),
        plan_std_output: apply_output.to_string(),
        plan_raw_json_key: mutate_raw_json_key,
        environment: payload.environment.clone(),
        change_type: payload.command.to_string(),
        resource_changes,
        variables: status_handler.get_variables(),
    };

    let _record_id = insert_infra_change_record(handler, infra_change_record)
        .await
        .context("Failed to insert infra change record after apply/destroy")?;

    Ok(())
}

#[tracing::instrument(skip_all, fields(cmd = %payload.command, module = %payload.module, version = %payload.module_version))]
pub async fn terraform_apply_destroy<'a>(
    payload: &'a ApiInfraPayload,
    handler: &GenericCloudHandler,
    status_handler: &mut DeploymentStatusHandler<'a>,
) -> Result<String, anyhow::Error> {
    let cmd = &payload.command;
    let deployment_id = &payload.deployment_id;
    let environment = &payload.environment;

    status_handler.set_command(cmd);

    match run_terraform_command(
        cmd,
        false,
        false,
        false,
        true,
        true,
        false,
        false,
        false,
        false,
        deployment_id,
        environment,
        50,
        Some(&get_extra_environment_variables(payload)),
    )
    .await
    {
        Ok(command_result) => {
            log::info!("Terraform {} successful", cmd);

            if cmd == "destroy" {
                status_handler.set_deleted(true);
            }

            Ok(command_result.stdout)
        }
        Err(e) => {
            log::info!("Error running \"terraform {}\" command: {:?}", cmd, e);
            let error_text = e.to_string();
            let status = DeploymentStatus::Error;
            status_handler.set_status(status);
            status_handler.set_event_duration();
            status_handler.set_error_text(error_text);
            status_handler.send_event(handler).await;
            status_handler.send_deployment(handler).await?;
            status_handler.set_error_text("".to_string());
            Err(anyhow!("Error running terraform {}: {}", cmd, e))
        }
    }
}

fn sanitize_terraform_output(mut output: Value) -> Value {
    if let Some(map) = output.as_object_mut() {
        for (_, v) in map.iter_mut() {
            if let Some(val_map) = v.as_object_mut() {
                if let Some(sensitive) = val_map.get("sensitive") {
                    if sensitive.as_bool().unwrap_or(false) {
                        val_map.insert(
                            "value".to_string(),
                            Value::String("(output sanitized)".to_string()),
                        );
                    }
                }
            }
        }
    }
    output
}

#[tracing::instrument(skip_all, fields(cmd = %payload.command, module = %payload.module, version = %payload.module_version))]
pub async fn terraform_output(
    payload: &ApiInfraPayload,
    handler: &GenericCloudHandler,
    status_handler: &mut DeploymentStatusHandler<'_>,
) -> Result<(), anyhow::Error> {
    let deployment_id = &payload.deployment_id;
    let environment = &payload.environment;
    let cmd = "output";

    match run_terraform_command(
        cmd,
        false,
        false,
        false,
        false,
        false,
        true,
        false,
        false,
        false,
        deployment_id,
        environment,
        10000,
        None,
    )
    .await
    {
        Ok(command_result) => {
            log::info!("Terraform {} successful", cmd);

            let output = match serde_json::from_str(command_result.stdout.as_str()) {
                Ok(json) => sanitize_terraform_output(json),
                Err(e) => {
                    return Err(anyhow!(
                        "Could not parse the terraform output json from stdout: {:?}\nString was:'{}'",
                        e,
                        command_result.stdout.as_str()
                    ));
                }
            };

            status_handler.set_status(DeploymentStatus::Successful);
            status_handler.set_output(output);
            status_handler.send_deployment(handler).await?;
        }
        Err(e) => {
            log::info!("Error: {:?}", e);

            let status = DeploymentStatus::FailedOutput;
            status_handler.set_status(status);
            status_handler.set_event_duration();
            status_handler.set_last_event_epoch(); // Reset the event duration timer for the next event
            status_handler.send_event(handler).await;
            status_handler.send_deployment(handler).await?;
        }
    }

    Ok(())
}

#[tracing::instrument(skip_all)]
pub async fn terraform_state_list() -> Result<Option<Vec<String>>, anyhow::Error> {
    let mut exec = tokio::process::Command::new("terraform");
    exec.arg("state")
        .arg("list")
        .arg("-no-color")
        .current_dir(Path::new("./"))
        .env("TF_CLI_CONFIG_FILE", terraform_cli_config_file())
        .env("TF_PLUGIN_CACHE_DIR", terraform_plugin_cache_dir())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());

    log::info!("Running terraform state list...");

    match run_generic_command(&mut exec, 10000, false).await {
        Ok(command_result) => {
            log::info!("Terraform state list successful");

            let resources: Vec<String> = command_result
                .stdout
                .lines()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();

            if resources.is_empty() {
                Ok(Some(Vec::with_capacity(0)))
            } else {
                Ok(Some(resources))
            }
        }
        Err(e) => {
            log::info!("Error getting terraform state list: {:?}", e);
            // Don't fail the entire deployment if we can't get the state list
            // Just return None and log the error
            Ok(None)
        }
    }
}

fn terraform_cli_config_file() -> String {
    env::var("TF_CLI_CONFIG_FILE").unwrap_or_else(|_| {
        if env::var("TEST_MODE").is_ok() {
            env::temp_dir()
                .join(".terraformrc")
                .to_string_lossy()
                .to_string()
        } else {
            "/app/.terraformrc".to_string()
        }
    })
}

fn terraform_plugin_cache_dir() -> String {
    env::var("TF_PLUGIN_CACHE_DIR").unwrap_or_else(|_| {
        if env::var("TEST_MODE").is_ok() {
            env::temp_dir()
                .join(".terraform-plugin-cache")
                .to_string_lossy()
                .to_string()
        } else {
            "/app/.terraform-plugin-cache".to_string()
        }
    })
}

#[tracing::instrument(skip_all)]
async fn download_all_providers(
    handler: &GenericCloudHandler,
    provider_versions: &[TfLockProvider],
    target: &str,
) -> Result<(), anyhow::Error> {
    let categories = ["provider_binary", "shasum", "signature"];

    let downloads = provider_versions
        .iter()
        .flat_map(|provider| {
            categories.iter().map(move |&category| async move {
                download_provider(handler, provider, target, category).await
            })
        })
        .collect::<Vec<_>>();

    let is_test_mode = std::env::var("TEST_MODE")
        .map(|val| val.to_lowercase() == "true" || val == "1")
        .unwrap_or(false);

    let concurrency_limit_env = std::env::var("CONCURRENCY_LIMIT")
        .unwrap_or_else(|_| "".to_string())
        .parse::<usize>()
        .unwrap_or(10);

    let effective_concurrency_limit = if is_test_mode {
        log::info!("TEST_MODE enabled, limiting all download operations to concurrency of 1");
        1
    } else {
        concurrency_limit_env
    };

    let results: Vec<Result<(), anyhow::Error>> = stream::iter(downloads)
        .buffer_unordered(effective_concurrency_limit)
        .collect()
        .await;

    for res in results {
        res?; // propagate any download error
    }
    Ok(())
}

#[tracing::instrument(skip_all, fields(provider = %tf_lock_provider.source, version = %tf_lock_provider.version))]
async fn download_provider(
    handler: &GenericCloudHandler,
    tf_lock_provider: &TfLockProvider,
    target: &str,
    category: &str,
) -> Result<()> {
    let mirror_dir = if std::env::var("TEST_MODE").is_ok() {
        env::temp_dir()
            .join(".provider-mirror")
            .to_string_lossy()
            .to_string()
    } else {
        "/app/.provider-mirror".to_string()
    };
    let (_url, s3_key) = get_provider_url_key(tf_lock_provider, target, category).await?;
    let destination = format!("{mirror_dir}/{s3_key}",);

    let dest_path = PathBuf::from(&destination);
    if dest_path.exists() {
        log::info!(
            "Provider artifact already exists at {}, reusing it",
            destination
        );
        return Ok(());
    }

    if let Some(parent) = dest_path.parent() {
        fs::create_dir_all(parent)
            .await
            .with_context(|| format!("Failed to create directory {:?}", parent))?;
    }

    let url = handler
        .generate_presigned_url(&s3_key, "providers")
        .await
        .with_context(|| format!("Failed to generate presigned URL for {}", s3_key))?;

    env_utils::download_zip(&url, &dest_path)
        .await
        .with_context(|| format!("Failed to download ZIP from {}", url))?;

    log::info!("Downloaded provider to {}", destination);
    Ok(())
}

#[tracing::instrument(skip_all)]
pub async fn set_up_provider_mirror(
    handler: &GenericCloudHandler,
    provider_versions: &[TfLockProvider],
    target: &str,
) -> Result<(), anyhow::Error> {
    let mirror_dir = if std::env::var("TEST_MODE").is_ok() {
        env::temp_dir()
            .join(".provider-mirror")
            .to_string_lossy()
            .to_string()
    } else {
        "/app/.provider-mirror".to_string()
    };
    fs::create_dir_all(&mirror_dir).await?;
    let plugin_cache_dir = terraform_plugin_cache_dir();
    fs::create_dir_all(&plugin_cache_dir).await?;

    let content = format!(
        r#"
plugin_cache_dir = "{}"

provider_installation {{
    # use the local filesystem mirror by default
    filesystem_mirror {{
        path    = "{}"
        include = ["*/*"]
    }}
    # use fallback for anything missing
    direct {{
        include = ["*/*"]
    }}
}}
"#,
        plugin_cache_dir, mirror_dir
    );
    let provider_mirror_file = terraform_cli_config_file();
    fs::write(&provider_mirror_file, content)
        .await
        .with_context(|| format!("Failed to write to {}", provider_mirror_file))?;
    log::info!("Provider mirror file created at {}", provider_mirror_file);

    download_all_providers(handler, provider_versions, target).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_sanitize_terraform_output() {
        let output = json!({
            "resource_name": {
                "sensitive": false,
                "type": "string",
                "value": "some-name-here"
            },
            "secret_password": {
                "sensitive": true,
                "type": "string",
                "value": "this_is_supersecret"
            }
        });

        let sanitized = sanitize_terraform_output(output);

        assert_eq!(sanitized["resource_name"]["value"], "some-name-here");
        assert_eq!(sanitized["secret_password"]["value"], "(output sanitized)");
    }
}
