use env_common::interface::GenericCloudHandler;
use env_common::logic::insert_infra_change_record;
use env_common::DeploymentStatusHandler;
use env_defs::{ApiInfraPayload, CloudProvider, ExtraData, InfraChangeRecord, TfLockProvider};
use env_utils::{get_epoch, get_provider_url_key, get_timestamp};
use futures::stream::{self, StreamExt};
use std::{
    env,
    path::{Path, PathBuf},
};
use tokio::fs;

use serde_json::Value;
use std::fs::{write, File};

use anyhow::{anyhow, Context, Result};
use env_aws::assume_role;

use crate::{post_webhook, run_generic_command, CommandResult};

#[allow(clippy::too_many_arguments)]
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
        .env("TF_CLI_CONFIG_FILE", "/app/.terraformrc")
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

    println!("Running terraform command:\n{:?}", exec.as_std());

    if init {
        GenericCloudHandler::default()
            .await
            .set_backend(&mut exec, deployment_id, environment)
            .await;
    }

    // TODO: Move this to env_common
    if env::var("AWS_ASSUME_ROLE_ARN").is_ok() {
        let assume_role_arn = env::var("AWS_ASSUME_ROLE_ARN").unwrap();
        match assume_role(
            &assume_role_arn,
            "infraweave-assume-during-terraform-command",
            3600,
        )
        .await
        {
            Ok(assumed_role_credentials) => {
                println!("Assumed role successfully");
                exec.env("AWS_ACCESS_KEY_ID", assumed_role_credentials.access_key_id);
                exec.env(
                    "AWS_SECRET_ACCESS_KEY",
                    assumed_role_credentials.secret_access_key,
                );
                exec.env("AWS_SESSION_TOKEN", assumed_role_credentials.session_token);
            }
            Err(e) => {
                println!("Error assuming role: {:?}", e);
                return Err(anyhow!("Error assuming role: {:?}", e));
            }
        }
    }

    run_generic_command(&mut exec, max_output_lines).await
}

pub fn store_tf_vars_json(tf_vars: &Value) {
    // Try to create a file
    let tf_vars_file = match File::create("terraform.tfvars.json") {
        Ok(file) => file,
        Err(e) => {
            eprintln!("Failed to create terraform.tfvars.json: {:?}", e);
            std::process::exit(1);
        }
    };

    // Write the JSON data to the file
    if let Err(e) = serde_json::to_writer_pretty(tf_vars_file, &tf_vars) {
        eprintln!("Failed to write JSON to terraform.tfvars.json: {:?}", e);
        std::process::exit(1);
    }

    println!("Terraform variables successfully stored in terraform.tfvars.json");
}

pub async fn store_backend_file() {
    // There are verifications when publishing a module to ensure that there
    // is no existing already backend specified. This is to ensure that InfraWeave
    // uses its backend storage
    let backend_file_content = format!(
        r#"
terraform {{
    backend "{}" {{}}
}}"#,
        GenericCloudHandler::default().await.get_backend_provider()
    );

    // Write the file content to the file
    let file_path = Path::new("backend.tf");
    if let Err(e) = write(file_path, &backend_file_content) {
        eprintln!("Failed to write to backend.tf: {:?}", e);
        std::process::exit(1);
    }

    println!("Terraform backend file successfully stored in backend.tf");
}

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
            println!("Terraform init successful");
            Ok(())
        }
        Err(e) => {
            println!("Error running \"terraform {}\" command: {:?}", cmd, e);
            let status = "failed_init".to_string();
            status_handler.set_status(status);
            status_handler.set_event_duration();
            status_handler.send_event(handler).await;
            status_handler.send_deployment(handler).await?;
            Err(anyhow!("Error running terraform init: {}", e))
        }
    }
}

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
            println!("Terraform {} successful", cmd);
            Ok(())
        }
        Err(e) => {
            println!("Error running \"terraform {}\" command: {:?}", cmd, e);
            let error_text: String = e.to_string();
            let status = "failed_validate".to_string();
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
            println!("Terraform plan successful");
            Ok(command_result.stdout)
        }
        Err(e) => {
            println!("Error running \"terraform plan\" command: {:?}", e);
            let error_text = e.to_string();
            let status = "failed_plan".to_string();
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

pub async fn terraform_show(
    payload: &ApiInfraPayload,
    job_id: &str,
    module: &env_defs::ModuleResp,
    plan_output: &str,
    handler: &GenericCloudHandler,
    status_handler: &mut DeploymentStatusHandler<'_>,
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
        true,
        false,
        deployment_id,
        environment,
        500,
        None,
    )
    .await
    {
        Ok(command_result) => {
            println!("Terraform {} successful", cmd);
            println!("Output: {}", command_result.stdout);

            let tf_plan = "./tf_plan.json";
            let tf_plan_file_path = Path::new(tf_plan);
            // Write the stdout content to the file without parsing to be used for OPA policy checks
            std::fs::write(tf_plan_file_path, &command_result.stdout)
                .expect("Unable to write to file");

            let content: Value = serde_json::from_str(command_result.stdout.as_str()).unwrap();

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
                                        println!("Webhook {:?} sent successfully", webhook);
                                    }
                                    Err(e) => {
                                        println!(
                                            "Error sending webhook: {:?} with url: {:?}",
                                            e, webhook
                                        );
                                        // Don't fail the deployment if the webhook fails
                                    }
                                }
                            }
                            None => {
                                println!("Webhook URL not provided");
                            }
                        }
                    }
                }
            }

            // Only create InfraChangeRecord for plan commands
            // For apply/destroy, the record is created after the operation in terraform_show_after_apply
            if command == "plan" {
                let plan_raw_json_key = format!(
                    "{}{}/{}/{}_{}_plan_output.json",
                    handler.get_storage_basepath(),
                    environment,
                    deployment_id,
                    command,
                    job_id
                );

                let infra_change_record = InfraChangeRecord {
                    deployment_id: deployment_id.to_string(),
                    project_id: project_id.clone(),
                    region: region.to_string(),
                    job_id: job_id.to_string(),
                    module: module.module.clone(),
                    module_version: module.version.clone(),
                    epoch: get_epoch(),
                    timestamp: get_timestamp(),
                    plan_std_output: plan_output.to_string(),
                    plan_raw_json_key,
                    environment: environment.clone(),
                    change_type: command.to_string(),
                };
                match insert_infra_change_record(
                    handler,
                    infra_change_record,
                    &command_result.stdout,
                )
                .await
                {
                    Ok(_) => {
                        println!("Infra change record for plan inserted");
                    }
                    Err(e) => {
                        println!("Error inserting infra change record: {:?}", e);
                    }
                }
            }

            Ok(())
        }
        Err(e) => {
            println!("Error running \"terraform {}\" command: {:?}", cmd, e);
            let error_text = e.to_string();
            let status = "failed_show_plan".to_string();
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

pub async fn terraform_show_after_apply(
    payload: &ApiInfraPayload,
    job_id: &str,
    module: &env_defs::ModuleResp,
    plan_output: &str,
    handler: &GenericCloudHandler,
    _status_handler: &mut DeploymentStatusHandler<'_>,
) -> Result<(), anyhow::Error> {
    let deployment_id = &payload.deployment_id;
    let environment = &payload.environment;
    let project_id = &payload.project_id;
    let region = &payload.region;
    let command = &payload.command;

    // Run terraform show without a planfile to get the current state
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
        false, // No plan input
        false,
        deployment_id,
        environment,
        500,
        None,
    )
    .await
    {
        Ok(command_result) => {
            println!("Terraform {} after apply successful", cmd);
            println!("Output: {}", command_result.stdout);

            let plan_raw_json_key = format!(
                "{}{}/{}/{}_{}_apply_output.json",
                handler.get_storage_basepath(),
                environment,
                deployment_id,
                command,
                job_id
            );

            let infra_change_record = InfraChangeRecord {
                deployment_id: deployment_id.to_string(),
                project_id: project_id.clone(),
                region: region.to_string(),
                job_id: job_id.to_string(),
                module: module.module.clone(),
                module_version: module.version.clone(),
                epoch: get_epoch(),
                timestamp: get_timestamp(),
                plan_std_output: plan_output.to_string(),
                plan_raw_json_key,
                environment: environment.clone(),
                change_type: command.to_string(),
            };
            match insert_infra_change_record(handler, infra_change_record, &command_result.stdout)
                .await
            {
                Ok(_) => {
                    println!("Infra change record for apply inserted");
                    Ok(())
                }
                Err(e) => {
                    println!("Error: {:?}", e);
                    Err(anyhow!(
                        "Error inserting infra change record after apply: {}",
                        e
                    ))
                }
            }
        }
        Err(e) => {
            println!(
                "Error running \"terraform {}\" after apply command: {:?}",
                cmd, e
            );
            println!("Warning: Failed to capture apply state, continuing...");
            Ok(())
        }
    }
}

#[rustfmt::skip]
fn get_extra_environment_variables(
    payload: &ApiInfraPayload,
) -> std::collections::HashMap<String, String> {
    let mut env_vars = std::collections::HashMap::new();
    env_vars.insert("INFRAWEAVE_DEPLOYMENT_ID".to_string(), payload.deployment_id.clone());
    env_vars.insert("INFRAWEAVE_ENVIRONMENT".to_string(), payload.environment.clone());
    env_vars.insert("INFRAWEAVE_REFERENCE".to_string(), payload.reference.clone());
    env_vars.insert("INFRAWEAVE_MODULE_VERSION".to_string(), payload.module_version.clone());
    env_vars.insert("INFRAWEAVE_MODULE_TYPE".to_string(), payload.module_type.clone());
    env_vars.insert("INFRAWEAVE_MODULE_TRACK".to_string(), payload.module_track.clone());
    env_vars.insert("INFRAWEAVE_DRIFT_DETECTION".to_string(), (if payload.drift_detection.enabled {"enabled"} else {"disabled"}).to_string());
    env_vars.insert("INFRAWEAVE_DRIFT_DETECTION_INTERVAL".to_string(), if payload.drift_detection.enabled {payload.drift_detection.interval.to_string()} else {"N/A".to_string()});

    match &payload.extra_data {
        ExtraData::GitHub(github_data) => {
            env_vars.insert("INFRAWEAVE_GIT_COMMITTER_EMAIL".to_string(), github_data.user.email.clone());
            env_vars.insert("INFRAWEAVE_GIT_COMMITTER_NAME".to_string(), github_data.user.name.clone());
            env_vars.insert("INFRAWEAVE_GIT_ACTOR_USERNAME".to_string(), github_data.user.username.clone());
            env_vars.insert("INFRAWEAVE_GIT_ACTOR_PROFILE_URL".to_string(), github_data.user.profile_url.clone());
            env_vars.insert("INFRAWEAVE_GIT_REPOSITORY_NAME".to_string(), github_data.repository.full_name.clone());
            env_vars.insert("INFRAWEAVE_GIT_REPOSITORY_PATH".to_string(), github_data.job_details.file_path.clone());
            env_vars.insert("INFRAWEAVE_GIT_COMMIT_SHA".to_string(), github_data.check_run.head_sha.clone());
        },  
        ExtraData::GitLab(gitlab_data) => {
            // TODO: Add more here for GitLab
            env_vars.insert("INFRAWEAVE_GIT_REPOSITORY_PATH".to_string(), gitlab_data.job_details.file_path.clone());
        },
        ExtraData::None => {}
    };
    env_vars
}

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
            println!("Terraform {} successful", cmd);

            if cmd == "destroy" {
                status_handler.set_deleted(true);
            }

            Ok(command_result.stdout)
        }
        Err(e) => {
            println!("Error running \"terraform {}\" command: {:?}", cmd, e);
            let error_text = e.to_string();
            let status = "error".to_string();
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

pub async fn terraform_output(
    payload: &ApiInfraPayload,
    handler: &GenericCloudHandler,
    status_handler: &mut DeploymentStatusHandler<'_>,
) -> Result<(), anyhow::Error> {
    let deployment_id = &payload.deployment_id;
    let environment = &payload.environment;
    let cmd = "output";

    status_handler.set_command(cmd);
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
        1000,
        None,
    )
    .await
    {
        Ok(command_result) => {
            println!("Terraform {} successful", cmd);
            println!("Output: {}", command_result.stdout);

            let output = match serde_json::from_str(command_result.stdout.as_str()) {
                Ok(json) => json,
                Err(e) => {
                    return Err(anyhow!(
                        "Could not parse the terraform output json from stdout: {:?}\nString was:'{}'",
                        e,
                        command_result.stdout.as_str()
                    ));
                }
            };

            status_handler.set_status("successful".to_string());
            status_handler.set_output(output);
            status_handler.send_deployment(handler).await?;
        }
        Err(e) => {
            println!("Error: {:?}", e);

            let status = "failed_output".to_string();
            status_handler.set_status(status);
            status_handler.set_event_duration();
            status_handler.set_last_event_epoch(); // Reset the event duration timer for the next event
            status_handler.send_event(handler).await;
            status_handler.send_deployment(handler).await?;
        }
    }

    Ok(())
}

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
        println!("TEST_MODE enabled, limiting all download operations to concurrency of 1");
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
    let (_url, s3_key) = get_provider_url_key(tf_lock_provider, target, category);
    let destination = format!("{mirror_dir}/{s3_key}",);

    let dest_path = PathBuf::from(&destination);
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

    println!("Downloaded provider to {}", destination);
    Ok(())
}

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

    let content = format!(
        r#"
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
        mirror_dir
    );
    let provider_mirror_file = if std::env::var("TEST_MODE").is_ok() {
        env::temp_dir()
            .join(".terraformrc")
            .to_string_lossy()
            .to_string()
    } else {
        "/app/.terraformrc".to_string()
    };
    fs::write(&provider_mirror_file, content)
        .await
        .with_context(|| format!("Failed to write to {}", provider_mirror_file))?;
    println!("Provider mirror file created at {}", provider_mirror_file);

    download_all_providers(handler, provider_versions, target).await?;
    Ok(())
}
