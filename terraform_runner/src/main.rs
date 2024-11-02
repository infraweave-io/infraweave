mod job_id;
mod read;

use anyhow::{anyhow, Result};
use env_common::interface::initialize_project_id;
use env_common::DeploymentStatusHandler;
use env_defs::{ApiInfraPayload, InfraChangeRecord, PolicyResult};
use env_utils::{get_epoch, get_timestamp};
use job_id::get_job_id;
use serde_json::{json, Value};
use std::collections::VecDeque;
use std::fs::{write, File};
use std::process::{exit, Command};
use std::vec;
use std::{env, path::Path};
use tokio::io::{AsyncBufReadExt, BufReader};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    initialize_project_id().await;

    let cloud = "aws";
    let cloud_handler: Box<dyn env_common::ModuleEnvironmentHandler> = match cloud {
        "azure" => Box::new(env_common::AzureHandler {}),
        "aws" => Box::new(env_common::AwsHandler {}),
        _ => panic!("Invalid cloud provider"),
    };

    let payload = get_payload();

    // print_all_environment_variables(); // DEBUG ONLY Remove this line

    println!("Storing terraform variables in tf_vars.json...");
    store_tf_vars_json(&payload.variables);
    store_backend_file();
    // cat_file("terraform.tfvars.json");

    println!("Read deployment id from environment variable...");

    let project_id = &payload.project_id;
    let region = &payload.region;

    let deployment_id = &payload.deployment_id;
    let environment = &payload.environment;
    let command = &payload.command;
    let refresh_only = payload.args.iter().any(|e| e == "-refresh-only");

    let error_text = "".to_string();
    let status = "initiated".to_string(); // received, initiated, completed, failed
    let job_id = match get_job_id().await {
        Ok(id) => id,
        Err(e) => {
            println!("Error: {:?}", e);
            panic!("Error getting job id");
        }
    };

    // To reduce clutter, a DeploymentStatusHandler is used to handle the status updates
    // since we will be updating the status multiple times and only a few fields change each time
    let mut status_handler = DeploymentStatusHandler::new(
        &command,
        &payload.module,
        &payload.module_version,
        &payload.module_type,
        status,
        &environment,
        &deployment_id,
        &project_id,
        &region,
        &error_text,
        &job_id,
        &payload.name,
        payload.variables.clone(),
        payload.drift_detection.clone(),
        payload.next_drift_check_epoch.clone(),
        payload.dependencies.clone(),
        Value::Null,
        vec![],
    );
    if command == "plan" && refresh_only {
        status_handler.set_is_drift_check();
    }
    status_handler.send_event().await;
    status_handler.send_deployment().await;

    if command == "apply" {
        // Check if all dependencies have state = finished, if not, store "waiting-on-dependency" status
        let mut dependencies_not_finished: Vec<env_defs::Dependency> = Vec::new();
        for dep in &payload.dependencies {
            let region = env::var("REGION").unwrap_or("eu-central-1".to_string());
            match check_dependency_status(
                &cloud_handler,
                dep.clone(),
                deployment_id,
                environment,
                &region,
            )
            .await
            {
                Ok(_) => {
                    println!("Dependency finished");
                }
                Err(e) => {
                    println!("Dependency not finished: {:?}", e);
                    dependencies_not_finished.push(dep.clone());
                }
            }
        }

        if dependencies_not_finished.len() > 0 {
            let status = "waiting-on-dependency".to_string();
            // status_handler.set_error_text(error_text);
            status_handler.set_status(status);
            status_handler.send_event().await;
            status_handler.send_deployment().await;
            exit(0);
        }
    } else if command == "destroy" {
        let (_, dependants) = cloud_handler
            .describe_deployment_id(deployment_id, environment)
            .await?;

        if dependants.len() > 0 {
            let status = "has-dependants".to_string();
            status_handler.set_error_text("This deployment has other deployments depending on it, and hence cannot be removed until they are removed");
            status_handler.set_status(status);
            status_handler.send_event().await;
            status_handler.send_deployment().await;
            exit(0);
        }
    }

    let module = get_module(&cloud_handler, &payload).await;
    download_module(&cloud_handler, &module.s3_key, "./").await;

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
        &deployment_id,
        &environment,
        50,
    )
    .await
    {
        Ok(_) => {
            println!("Terraform init successful");
        }
        Err(e) => {
            println!("Error running \"terraform {}\" command: {:?}", cmd, e);
            let status = "failed_init".to_string();
            status_handler.set_status(status);
            status_handler.send_event().await;
            status_handler.send_deployment().await;
            exit(0);
        }
    }

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
        &deployment_id,
        &environment,
        50,
    )
    .await
    {
        Ok(_) => {
            println!("Terraform {} successful", cmd);
        }
        Err(e) => {
            println!("Error running \"terraform {}\" command: {:?}", cmd, e);
            let error_text = e.to_string();
            let status = "failed_validate".to_string();
            status_handler.set_status(status);
            status_handler.set_error_text(&error_text);
            status_handler.send_event().await;
            status_handler.send_deployment().await;
            status_handler.set_error_text("");
            exit(0);
        }
    }

    let mut plan_output = "".to_string();

    let cmd = "plan";
    match run_terraform_command(
        cmd,
        refresh_only,
        command == "plan",
        command == "destroy",
        false,
        false,
        false,
        true,
        false,
        false,
        &deployment_id,
        &environment,
        500,
    )
    .await
    {
        Ok(command_result) => {
            println!("Terraform {} successful", cmd);
            plan_output = command_result.stdout;
        }
        Err(e) => {
            println!("Error running \"terraform {}\" command: {:?}", cmd, e);
            let error_text = e.to_string();
            let status = "failed_plan".to_string();
            status_handler.set_status(status);
            status_handler.set_error_text(&error_text);
            status_handler.send_event().await;
            status_handler.send_deployment().await;
            status_handler.set_error_text("");
            exit(0);
        }
    }

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
        &deployment_id,
        &environment,
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

            let content: Value = serde_json::from_str(&command_result.stdout.as_str()).unwrap();

            if command == "plan" && refresh_only {
                let drift_has_occurred = content.get("resource_drift").unwrap_or(&serde_json::from_str("[]").unwrap()).as_array().unwrap().len() > 0;
                status_handler.set_drift_has_occurred(drift_has_occurred);
            }

            let account_id = get_env_var("ACCOUNT_ID");
            let plan_raw_json_key = format!(
                "{}/{}/{}/{}_{}_plan_output.json",
                account_id, environment, deployment_id, command, &job_id
            );

            let infra_change_record = InfraChangeRecord {
                deployment_id: deployment_id.clone(),
                project_id: project_id.clone(),
                region: region.clone(),
                job_id: job_id.to_string(),
                module: module.module.clone(),
                module_version: module.version.clone(),
                epoch: get_epoch(),
                timestamp: get_timestamp(),
                plan_std_output: plan_output.clone(),
                plan_raw_json_key: plan_raw_json_key,
                environment: environment.clone(),
                change_type: command.to_string(),
            };
            match &cloud_handler
                .insert_infra_change_record(infra_change_record, &command_result.stdout)
                .await
            {
                Ok(_) => {
                    println!("Infra change record inserted");
                }
                Err(e) => {
                    println!("Error: {:?}", e);
                    panic!("Error inserting infra change record");
                }
            }
        }
        Err(e) => {
            println!("Error running \"terraform {}\" command: {:?}", cmd, e);
            let error_text = e.to_string();
            let status = "failed_show_plan".to_string();
            status_handler.set_status(status);
            status_handler.set_error_text(&error_text);
            status_handler.send_event().await;
            status_handler.send_deployment().await;
            status_handler.set_error_text("");
            exit(0);
        }
    }

    println!("Prepare for OPA policy checks...");

    // Store specific environment variables in a JSON file to be used by OPA policies
    let file_path = "./env_data.json";
    match store_env_as_json(file_path) {
        Ok(_) => println!("Environment variables stored in {}.", file_path),
        Err(e) => eprintln!("Failed to write file: {}", e),
    }

    println!("Finding all applicable policies...");
    let policies = cloud_handler
        .list_policy("dev")
        .await
        .unwrap();

    let mut policy_results: Vec<PolicyResult> = vec![];
    let mut failed_policy_evaluation = false;

    println!("Running OPA policy checks...");
    for policy in policies {
        download_policy(&cloud_handler, &policy).await;

        // Store policy input in a JSON file
        let policy_input_file = "./policy_input.json";
        let policy_input_file_path = Path::new(policy_input_file);
        let policy_input_file = File::create(policy_input_file_path).unwrap();
        serde_json::to_writer(policy_input_file, &policy.data).unwrap();

        let rego_files: Vec<String> = get_all_rego_filenames_in_cwd();

        match run_opa_command(500, &policy.policy, &rego_files).await {
            Ok(command_result) => {
                println!("OPA policy evaluation for {} finished", &policy.policy);

                let opa_result: Value = match serde_json::from_str(command_result.stdout.as_str()) {
                    Ok(json) => json,
                    Err(e) => {
                        panic!("Could not parse the opa output json from stdout: {:?}\nString was:'{:?}", e, command_result.stdout.as_str());
                    }
                };

                // == opa_result example: ==
                //  {
                //     "helpers": {},
                //     "terraform_plan": {
                //       "deny": [
                //         "Invalid region: 'eu-central-1'. The allowed AWS regions are: [\"us-east-1\", \"eu-west-1\"]"
                //       ]
                //     }
                //  }
                // =========================

                let mut failed: bool = false;
                let mut policy_violations: Value = json!({});
                for (opa_package_name, value) in opa_result.as_object().unwrap() {
                    if let Some(violations) = value.get("deny") {
                        if violations.as_array().unwrap().len() > 0 {
                            failed = true;
                            failed_policy_evaluation = true;
                            policy_violations[opa_package_name] = violations.clone();

                            // println!("Policy violations found for policy: {}", policy.policy);
                            // println!("Violations: {}", violations);
                            // println!("Current rego files for further information:");
                            // cat_file("./tf_plan.json"); // BE CARFEFUL WITH THIS LINE, CAN EXPOSE SENSITIVE DATA
                            // cat_file("./env_data.json");
                            // cat_file("./policy_input.json");
                            // for file in &rego_files {
                            //     cat_file(file);
                            // }
                        }
                    }
                }
                policy_results.push(PolicyResult {
                    policy: policy.policy.clone(),
                    version: policy.version.clone(),
                    environment: policy.environment.clone(),
                    description: policy.description.clone(),
                    policy_name: policy.policy_name.clone(),
                    failed: failed,
                    violations: policy_violations,
                });
            }
            Err(e) => {
                println!(
                    "Error running OPA policy evaluation command for {}",
                    policy.policy
                ); // TODO: use stderr from command_result
                let error_text = e.to_string();
                let status = "failed_policy".to_string();
                status_handler.set_status(status);
                status_handler.set_error_text(&error_text);
                status_handler.send_event().await;
                status_handler.send_deployment().await;
                status_handler.set_error_text("");
                exit(0);
            }
        }

        // Delete rego files after each policy check to avoid conflicts
        for rego_file in &rego_files {
            std::fs::remove_file(rego_file).unwrap();
        }
    }

    status_handler.set_policy_results(policy_results);

    if failed_policy_evaluation {
        println!("Error: OPA Policy evaluation found policy violations, aborting deployment");
        let status = "failed_policy".to_string();
        status_handler.set_status(status);
        status_handler.send_event().await;
        status_handler.send_deployment().await;
        exit(0);
    }

    if command == "apply" || command == "destroy" {
        let cmd = command; // from payload.command
        status_handler.set_command(&cmd);
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
            &deployment_id,
            &environment,
            50,
        )
        .await
        {
            Ok(_) => {
                println!("Terraform {} successful", cmd);

                let status = "successful".to_string();
                status_handler.set_status(status);
                if cmd == "destroy" {
                    status_handler.set_deleted(true);
                }
                status_handler.send_event().await;
                status_handler.send_deployment().await;
            }
            Err(e) => {
                println!("Error running \"terraform {}\" command: {:?}", cmd, e);
                let error_text = e.to_string();
                let status = "error".to_string();
                status_handler.set_status(status);
                status_handler.set_error_text(&error_text);
                status_handler.send_event().await;
                status_handler.send_deployment().await;
                status_handler.set_error_text("");
                exit(0);
            }
        }

        if command == "apply" {
            let cmd = "output";
            status_handler.set_command(&cmd);
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
                &deployment_id,
                &environment,
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
                            panic!("Could not parse the terraform output json from stdout: {:?}\nString was:'{:?}", e, command_result.stdout.as_str());
                        }
                    };

                    status_handler.set_output(output);
                    status_handler.send_deployment().await;
                }
                Err(e) => {
                    println!("Error: {:?}", e);

                    let status = "failed_output".to_string();
                    status_handler.set_status(status);
                    status_handler.send_event().await;
                    status_handler.send_deployment().await;
                }
            }
        }
    } else if command == "plan" {
        status_handler.set_status("successful".to_string());
        status_handler.send_event().await;
        status_handler.send_deployment().await;
    }

    println!("Done!");

    Ok(())
}

async fn get_module(
    cloud_handler: &Box<dyn env_common::ModuleEnvironmentHandler>,
    payload: &ApiInfraPayload,
) -> env_defs::ModuleResp {
    let environment = "dev".to_string(); // &payload.environment;
    match cloud_handler
        .get_module_version(&payload.module, &environment, &payload.module_version)
        .await
    {
        Ok(module) => {
            println!("Module exists: {:?}", module);
            if module.is_none() {
                panic!("Module does not exist");
            }
            module.unwrap()
        }
        Err(e) => {
            println!("Module does not exist: {:?}", e);
            panic!("Module does not exist"); // TODO: handle this error and set status to failed
        }
    }
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
            std::process::exit(1); // Exit if parsing fails
        }
    };
    payload
}

fn store_tf_vars_json(tf_vars: &Value) {
    // Try to create a file and write the JSON data to it
    let tf_vars_file = match File::create("terraform.tfvars.json") {
        Ok(file) => file,
        Err(e) => {
            eprintln!("Failed to create terraform.tfvars.json: {:?}", e);
            std::process::exit(1); // Exit if file creation fails
        }
    };

    // Write the JSON data to the file
    if let Err(e) = serde_json::to_writer_pretty(tf_vars_file, &tf_vars) {
        eprintln!("Failed to write JSON to terraform.tfvars.json: {:?}", e);
        std::process::exit(1); // Exit if writing fails
    }

    println!("Terraform variables successfully stored in terraform.tfvars.json");
}

// There are verifications when publishing a module to ensure that there is no existing backend specified
fn store_backend_file() { // TODO: store this as env-var for different cloud providers and store in this function
    let backend_file_content = format!(
        r#"terraform {{
            backend "s3" {{}}
        }}"#);

    // Write the file content to the file
    let file_path = Path::new("backend.tf");
    if let Err(e) = write(file_path, &backend_file_content) {
        eprintln!("Failed to write to backend.tf: {:?}", e);
        std::process::exit(1); // Exit if writing fails
    }

    println!("Terraform backend file successfully stored in backend.tf");
}

fn print_all_environment_variables() {
    for (key, value) in env::vars() {
        println!("{}: {}", key, value);
    }
}

fn store_env_as_json(file_path: &str) -> std::io::Result<()> {
    let aws_default_region = env::var("AWS_DEFAULT_REGION").unwrap_or_else(|_| "".to_string());
    let aws_region = env::var("AWS_REGION").unwrap_or_else(|_| "".to_string());

    let env_vars = json!({
        "env": {
            "AWS_DEFAULT_REGION": aws_default_region,
            "AWS_REGION": aws_region
        }
    });

    let env_file_path = Path::new(file_path);
    let env_file = File::create(env_file_path).unwrap();
    serde_json::to_writer(env_file, &env_vars).unwrap();

    Ok(())
}

fn cat_file(filename: &str) {
    println!("=== File content: {} ===", filename);
    let output = Command::new("cat")
        .arg(filename)
        .output()
        .expect("Failed to execute command");

    println!("{}", String::from_utf8_lossy(&output.stdout));
}

async fn run_terraform_command(
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
) -> Result<(CommandResult), anyhow::Error> {
    let mut exec = tokio::process::Command::new("terraform");
    exec.arg(command)
        .arg("-no-color")
        .current_dir(&Path::new("./"))
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

    if no_lock_flag { // Allow multiple plans to be run in parallel, without locking the state
        exec.arg("-lock=false");
    }

    println!("Running terraform command: {:?}", exec);

    if init {
        let account_id = get_env_var("ACCOUNT_ID");
        let tf_bucket = get_env_var("TF_BUCKET");
        // let environment = get_env_var("ENVIRONMENT");
        let region = get_env_var("REGION");
        let key = format!(
            "{}/{}/{}/terraform.tfstate",
            account_id, environment, deployment_id
        );
        let dynamodb_table = get_env_var("TF_DYNAMODB_TABLE");
        exec.arg(format!("-backend-config=bucket={}", tf_bucket));
        exec.arg(format!("-backend-config=key={}", key));
        exec.arg(format!("-backend-config=region={}", region));
        exec.arg(format!("-backend-config=dynamodb_table={}", dynamodb_table));
    }

    run_generic_command(&mut exec, max_output_lines).await
}

async fn run_opa_command(
    max_output_lines: usize,
    policy_name: &str,
    rego_files: &Vec<String>,
) -> Result<(CommandResult), anyhow::Error> {
    println!("Running opa eval on policy {}", policy_name);

    let mut exec = tokio::process::Command::new("opa");
    exec.arg("eval").arg("--format").arg("pretty");

    for rego_file in rego_files {
        println!("Adding arg to opa command --data {}", rego_file);
        exec.arg("--data");
        exec.arg(rego_file);
    }

    exec.arg("--input")
        .arg("./tf_plan.json")
        .arg("--data")
        .arg("./env_data.json")
        .arg("--data")
        .arg("./policy_input.json")
        .arg("data.marius")
        .current_dir(&Path::new("./"))
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped()); // Capture stdout

    println!("Running opa command...");
    // Print command
    println!("{:?}", exec);

    run_generic_command(&mut exec, max_output_lines).await
}

struct CommandResult {
    stdout: String,
    stderr: String,
}

async fn run_generic_command(
    exec: &mut tokio::process::Command,
    max_output_lines: usize,
) -> Result<(CommandResult), anyhow::Error> {
    let mut child = exec.spawn()?; // Start the command without waiting for it to finish
                                   // Check if `stdout` was successfully captured

    let stdout = child.stdout.take().expect("Failed to capture stdout");
    let stderr = child.stderr.take().expect("Failed to capture stderr");

    let mut stdout_reader = BufReader::new(stdout).lines();
    let mut stderr_reader = BufReader::new(stderr).lines();

    let mut last_stdout_lines = VecDeque::new();
    let mut last_stderr_lines = VecDeque::new();

    let mut stdout_done = false;
    let mut stderr_done = false;

    while !stdout_done || !stderr_done {
        tokio::select! {
            stdout_line = stdout_reader.next_line(), if !stdout_done => {
                match stdout_line {
                    Ok(Some(line)) => {
                        println!("{}", line); // Print each line to stdout
                        // Collect the line into the buffer
                        last_stdout_lines.push_back(line);
                        if last_stdout_lines.len() > max_output_lines {
                            last_stdout_lines.pop_front(); // Keep only the last N lines
                        }
                    },
                    Ok(None) => {
                        stdout_done = true; // EOF on stdout
                    },
                    Err(e) => {
                        eprintln!("Error reading stdout: {}", e);
                        stdout_done = true;
                    },
                }
            },
            stderr_line = stderr_reader.next_line(), if !stderr_done => {
                match stderr_line {
                    Ok(Some(line)) => {
                        // Collect the line into the buffer
                        last_stderr_lines.push_back(line);
                        if last_stderr_lines.len() > max_output_lines {
                            last_stderr_lines.pop_front(); // Keep only the last N lines
                        }
                    },
                    Ok(None) => {
                        stderr_done = true; // EOF on stderr
                    },
                    Err(e) => {
                        eprintln!("Error reading stderr: {}", e);
                        stderr_done = true;
                    },
                }
            },
        }
    }

    let exist_status = child.wait().await?;

    let stderr_text = last_stderr_lines
        .iter()
        .fold(String::new(), |acc, line| acc + line.as_str() + "\n");
    if !exist_status.success() {
        return Err(anyhow!(stderr_text));
    }

    let stdout_text = last_stdout_lines
        .iter()
        .fold(String::new(), |acc, line| acc + line.as_str() + "\n");
    Ok(CommandResult {
        stdout: stdout_text,
        stderr: stderr_text,
    })
}

fn get_env_var(key: &str) -> String {
    match env::var(key) {
        Ok(val) => val,
        Err(_) => {
            eprintln!("Environment variable {} is not set", key);
            std::process::exit(1);
        }
    }
}

async fn download_module(
    cloud_handler: &Box<dyn env_common::ModuleEnvironmentHandler>,
    s3_key: &String,
    destination: &str,
) {
    println!("Downloading module from {}...", s3_key);

    let url = match cloud_handler.get_module_download_url(s3_key).await {
        Ok(url) => url,
        Err(e) => {
            panic!("Error: {:?}", e);
        }
    };

    match env_utils::download_zip(&url, &Path::new("module.zip")).await {
        Ok(_) => {
            println!("Downloaded module");
        }
        Err(e) => {
            panic!("Error: {:?}", e);
        }
    }

    match env_utils::unzip_file(&Path::new("module.zip"), &Path::new(destination)) {
        Ok(_) => {
            println!("Unzipped module");
        }
        Err(e) => {
            panic!("Error: {:?}", e);
        }
    }
}

async fn download_policy(
    cloud_handler: &Box<dyn env_common::ModuleEnvironmentHandler>,
    policy: &env_defs::PolicyResp,
) {
    println!("Downloading policy for {}...", policy.policy);

    let url = match cloud_handler.get_policy_download_url(&policy.s3_key).await {
        Ok(url) => url,
        Err(e) => {
            panic!("Error: {:?}", e);
        }
    };

    match env_utils::download_zip(&url, &Path::new("policy.zip")).await {
        Ok(_) => {
            println!("Downloaded policy successfully");
        }
        Err(e) => {
            panic!("Error: {:?}", e);
        }
    }

    let metadata = std::fs::metadata("policy.zip").unwrap();
    println!("Size of policy.zip: {:?} bytes", metadata.len());

    match env_utils::unzip_file(&Path::new("policy.zip"), &Path::new("./")) {
        Ok(_) => {
            println!("Unzipped policy successfully");
        }
        Err(e) => {
            panic!("Error: {:?}", e);
        }
    }
}

fn update_deployment(deployment: env_defs::DeploymentResp) {
    println!("Updating deployment {}...", deployment.deployment_id);
}

fn create_deployment() {
    println!("Creating deployment...");
}

async fn check_dependency_status(
    cloud_handler: &Box<dyn env_common::ModuleEnvironmentHandler>,
    dependency: env_defs::Dependency,
    deployment_id: &String,
    environment: &String,
    region: &String,
) -> Result<(), anyhow::Error> {
    println!("Checking dependency status...");
    match cloud_handler
        .describe_deployment_id(deployment_id, environment)
        .await
    {
        Ok((deployment, dependents)) => {
            if deployment.status == "finished" {
                return Ok(());
            } else {
                return Err(anyhow!("Dependency not finished"));
            }
        }

        Err(e) => {
            println!("Error: {:?}", e);
            panic!("Error getting deployment status");
        }
    };
}

fn get_all_rego_filenames_in_cwd() -> Vec<String> {
    let rego_files: Vec<String> = std::fs::read_dir("./")
        .unwrap()
        .filter_map(|entry| entry.ok())
        .filter(|entry| {
            entry
                .file_name()
                .to_str()
                .map(|s| s.ends_with(".rego"))
                .unwrap_or(false)
        })
        .map(|entry| entry.path().to_str().unwrap().to_string())
        .collect();
    rego_files
}
