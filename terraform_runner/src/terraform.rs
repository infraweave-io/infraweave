use crate::{run_generic_command, CommandResult};
use anyhow::{anyhow, Result};
use env_aws::assume_role;
use env_common::interface::GenericCloudHandler;
use env_defs::CloudProvider;
use std::{env, path::Path};

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
