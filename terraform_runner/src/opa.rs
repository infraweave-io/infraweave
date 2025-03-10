use env_common::interface::GenericCloudHandler;
use env_common::DeploymentStatusHandler;
use env_defs::{CloudProvider, PolicyResult};
use serde_json::{json, Value};
use std::{env, fs::File, path::Path, process::exit};

use crate::cmd::{run_generic_command, CommandResult};

pub async fn run_opa_command(
    max_output_lines: usize,
    policy_name: &str,
    rego_files: &Vec<String>,
) -> Result<CommandResult, anyhow::Error> {
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
        .arg("data.infraweave")
        .current_dir(Path::new("./"))
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped()); // Capture stdout

    println!("Running opa command...");
    // Print command
    println!("{:?}", exec);

    run_generic_command(&mut exec, max_output_lines).await
}

pub async fn download_policy(policy: &env_defs::PolicyResp) {
    println!("Downloading policy for {}...", policy.policy);

    let handler = GenericCloudHandler::default().await;
    let url = match handler.get_policy_download_url(&policy.s3_key).await {
        Ok(url) => url,
        Err(e) => {
            panic!("Error: {:?}", e);
        }
    };

    match env_utils::download_zip(&url, Path::new("policy.zip")).await {
        Ok(_) => {
            println!("Downloaded policy successfully");
        }
        Err(e) => {
            panic!("Error: {:?}", e);
        }
    }

    let metadata = std::fs::metadata("policy.zip").unwrap();
    println!("Size of policy.zip: {:?} bytes", metadata.len());

    match env_utils::unzip_file(Path::new("policy.zip"), Path::new("./")) {
        Ok(_) => {
            println!("Unzipped policy successfully");
        }
        Err(e) => {
            panic!("Error: {:?}", e);
        }
    }
}

pub fn get_all_rego_filenames_in_cwd() -> Vec<String> {
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

pub async fn run_opa_policy_checks<'a>(
    handler: &GenericCloudHandler,
    status_handler: &mut DeploymentStatusHandler<'a>,
) -> Result<(), anyhow::Error> {
    // Store specific environment variables in a JSON file to be used by OPA policies
    let file_path = "./env_data.json";
    match store_env_as_json(file_path) {
        Ok(_) => println!("Environment variables stored in {}.", file_path),
        Err(e) => eprintln!("Failed to write file: {}", e),
    }

    let policy_environment = "stable".to_string();
    println!(
        "Finding all applicable policies for {}...",
        &policy_environment
    );
    let policies = handler.get_all_policies(&policy_environment).await.unwrap();

    let mut policy_results: Vec<PolicyResult> = vec![];
    let mut failed_policy_evaluation = false;

    println!("Running OPA policy checks...");
    for policy in policies {
        download_policy(&policy).await;

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
                        if !violations.as_array().unwrap().is_empty() {
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
                    failed,
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
                status_handler.set_event_duration();
                status_handler.set_error_text(error_text);
                status_handler.send_event(handler).await;
                status_handler.send_deployment(handler).await;
                status_handler.set_error_text("".to_string());
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
        status_handler.set_event_duration();
        status_handler.send_event(handler).await;
        status_handler.send_deployment(handler).await;
        return Err(anyhow::anyhow!(
            "OPA Policy evaluation found policy violations"
        ));
    }

    Ok(())
}

fn store_env_as_json(file_path: &str) -> std::io::Result<()> {
    // Important for OPA policy checks
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
