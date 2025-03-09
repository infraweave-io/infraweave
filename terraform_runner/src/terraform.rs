use env_common::interface::GenericCloudHandler;
use env_common::logic::insert_infra_change_record;
use env_common::DeploymentStatusHandler;
use env_defs::{ApiInfraPayload, CloudProvider, InfraChangeRecord};
use env_utils::{get_epoch, get_timestamp};
use std::{env, path::Path};

use serde_json::Value;
use std::fs::{write, File};

use anyhow::{anyhow, Result};
use env_aws::assume_role;

use crate::{get_env_var, post_webhook, run_generic_command, CommandResult};

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
) -> Result<CommandResult, anyhow::Error> {
    let mut exec = tokio::process::Command::new("terraform");
    exec.arg(command)
        .arg("-no-color")
        .current_dir(Path::new("./"))
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped()); // Capture stdout

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

    println!("Running terraform command: {:?}", exec);

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
            status_handler.send_deployment(handler).await;
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
            status_handler.send_deployment(handler).await;
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
    let refresh_only = payload.args.iter().any(|e| e == "-refresh-only");

    match run_terraform_command(
        "plan",
        refresh_only,
        command == "plan",
        command == "destroy",
        false,
        false,
        false,
        true,
        false,
        false,
        deployment_id,
        environment,
        500,
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
            status_handler.send_deployment(handler).await;
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
    let refresh_only = payload.args.iter().any(|e| e == "-refresh-only");

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

            let account_id = get_env_var("ACCOUNT_ID");
            let plan_raw_json_key = format!(
                "{}/{}/{}/{}_{}_plan_output.json",
                account_id, environment, deployment_id, command, job_id
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
                    println!("Infra change record inserted");
                    Ok(())
                }
                Err(e) => {
                    println!("Error: {:?}", e);
                    Err(anyhow!("Error inserting infra change record: {}", e))
                }
            }
        }
        Err(e) => {
            println!("Error running \"terraform {}\" command: {:?}", cmd, e);
            let error_text = e.to_string();
            let status = "failed_show_plan".to_string();
            status_handler.set_status(status);
            status_handler.set_event_duration();
            status_handler.set_error_text(error_text);
            status_handler.send_event(handler).await;
            status_handler.send_deployment(handler).await;
            status_handler.set_error_text("".to_string());
            Err(anyhow!("Error running terraform show: {}", e))
        }
    }
}

pub async fn terraform_apply_destroy<'a>(
    payload: &'a ApiInfraPayload,
    handler: &GenericCloudHandler,
    status_handler: &mut DeploymentStatusHandler<'a>,
) -> Result<(), anyhow::Error> {
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
    )
    .await
    {
        Ok(_) => {
            println!("Terraform {} successful", cmd);

            let status = "successful".to_string();
            status_handler.set_status(status);
            status_handler.set_event_duration();
            status_handler.set_last_event_epoch(); // Reset the event duration timer for the next event
            if cmd == "destroy" {
                status_handler.set_deleted(true);
            }
            status_handler.send_event(handler).await;
            status_handler.send_deployment(handler).await;
            Ok(())
        }
        Err(e) => {
            println!("Error running \"terraform {}\" command: {:?}", cmd, e);
            let error_text = e.to_string();
            let status = "error".to_string();
            status_handler.set_status(status);
            status_handler.set_event_duration();
            status_handler.set_error_text(error_text);
            status_handler.send_event(handler).await;
            status_handler.send_deployment(handler).await;
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
            status_handler.send_deployment(handler).await;
        }
        Err(e) => {
            println!("Error: {:?}", e);

            let status = "failed_output".to_string();
            status_handler.set_status(status);
            status_handler.set_event_duration();
            status_handler.set_last_event_epoch(); // Reset the event duration timer for the next event
            status_handler.send_event(handler).await;
            status_handler.send_deployment(handler).await;
        }
    }

    Ok(())
}
