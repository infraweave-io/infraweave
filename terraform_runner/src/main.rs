mod read;
mod webhook;

use anyhow::{anyhow, Result};
use env_common::interface::{initialize_project_id_and_region, GenericCloudHandler};
use env_common::logic::{driftcheck_infra, insert_infra_change_record};
use env_common::{get_module_download_url, DeploymentStatusHandler};
use env_defs::{
    ApiInfraPayload, CloudProvider, Dependency, Dependent, InfraChangeRecord, PolicyResult,
};
use env_utils::{get_epoch, get_timestamp, setup_logging};
use futures::future::join_all;
use log::{error, info};
use serde_json::{json, Value};
use std::fs::{write, File};
use std::process::exit;
use std::vec;
use std::{env, path::Path};
use terraform_runner::{
    download_policy, get_all_rego_filenames_in_cwd, run_opa_command, run_terraform_command,
};
use webhook::post_webhook;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    setup_logging().expect("Failed to initialize logging.");
    initialize_project_id_and_region().await;

    let handler = GenericCloudHandler::default().await;

    let payload = get_payload();

    // print_all_environment_variables(); // DEBUG ONLY Remove this line

    if env::var("AZURE_CONTAINER_INSTANCE").is_ok() {
        // TODO: Move this?
        // Following is necessary since the oauth2 endpoint takes some time to be ready in Azure Container Instances
        println!("Running in Azure Container Instance, waiting for network to be ready...");
        std::thread::sleep(std::time::Duration::from_secs(25)); // TODO: Replace with a loop that checks if the endpoint is ready
        println!("Network should be ready now");
    };

    println!("Storing terraform variables in tf_vars.json...");
    store_tf_vars_json(&payload.variables);
    store_backend_file().await;

    println!("Read deployment id from environment variable...");

    let project_id = &payload.project_id;
    let region = &payload.region;

    let deployment_id = &payload.deployment_id;
    let environment = &payload.environment;
    let command = &payload.command;
    let refresh_only = payload.args.iter().any(|e| e == "-refresh-only");
    let initiated_by = &payload.initiated_by;

    let error_text = "".to_string();
    let status = "initiated".to_string(); // received, initiated, completed, failed
    let job_id = "unknown_jobid".to_string();

    let (initial_deployment, _dependents) = match handler
        .get_deployment_and_dependents(deployment_id, environment, false)
        .await
    {
        Ok((deployment, dependents)) => match deployment {
            Some(deployment) => {
                println!("Deployment found: {:?}", deployment);
                (Some(deployment), dependents)
            }
            None => {
                println!("Deployment not found");
                (None, dependents)
            }
        },
        Err(e) => Err(anyhow!("Error getting deployment and dependents: {}", e))?,
    };

    // To reduce clutter, a DeploymentStatusHandler is used to handle the status updates
    // since we will be updating the status multiple times and only a few fields change each time
    let mut status_handler = DeploymentStatusHandler::new(
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
        &error_text,
        &job_id,
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
        if initial_deployment.is_some() {
            initial_deployment.unwrap().policy_results
        } else {
            vec![]
        },
        initiated_by,
        payload.cpu.clone(),
        payload.memory.clone(),
        payload.reference.clone(),
    );

    let job_id = match handler.get_current_job_id().await {
        Ok(id) => id,
        Err(e) => {
            println!("Error: {:?}", e);
            let status = "failed".to_string();
            status_handler
                .set_error_text("The job failed to fetch the job id, please retry again.");
            status_handler.set_status(status);
            status_handler.set_event_duration();
            status_handler.set_last_event_epoch(); // Reset the event duration timer for the next event
            status_handler.send_event(&handler).await;
            status_handler.send_deployment(&handler).await;
            panic!("Error getting job id");
        }
    };
    status_handler.set_job_id(&job_id);

    if command == "plan" && refresh_only {
        status_handler.set_is_drift_check();
    }
    status_handler.send_event(&handler).await;
    status_handler.set_last_event_epoch(); // Initiate the event duration timer
    status_handler.send_deployment(&handler).await;

    if command == "apply" {
        // Check if all dependencies have state = finished, if not, store "waiting-on-dependency" status
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
            status_handler.set_last_event_epoch(); // Reset the event duration timer for the next event
            status_handler.send_event(&handler).await;
            status_handler.send_deployment(&handler).await;
            return Ok(()); // Exit since we are waiting on dependencies
        }
    } else if command == "destroy" {
        let (_, dependants) = handler
            .get_deployment_and_dependents(deployment_id, environment, false)
            .await?;

        if !dependants.is_empty() {
            let status = "has-dependants".to_string();
            status_handler.set_error_text("This deployment has other deployments depending on it, and hence cannot be removed until they are removed");
            status_handler.set_status(status);
            status_handler.set_event_duration();
            status_handler.set_last_event_epoch(); // Reset the event duration timer for the next event
            status_handler.send_event(&handler).await;
            status_handler.send_deployment(&handler).await;
            exit(0);
        }
    }

    let module = get_module(&payload).await;
    download_module(&module.s3_key, "./").await;

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
        }
        Err(e) => {
            println!("Error running \"terraform {}\" command: {:?}", cmd, e);
            let status = "failed_init".to_string();
            status_handler.set_status(status);
            status_handler.set_event_duration();
            status_handler.set_last_event_epoch(); // Reset the event duration timer for the next event
            status_handler.send_event(&handler).await;
            status_handler.send_deployment(&handler).await;
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
        deployment_id,
        environment,
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
            status_handler.set_event_duration();
            status_handler.set_last_event_epoch(); // Reset the event duration timer for the next event
            status_handler.set_error_text(&error_text);
            status_handler.send_event(&handler).await;
            status_handler.send_deployment(&handler).await;
            status_handler.set_error_text("");
            exit(0);
        }
    }

    #[allow(unused_assignments)]
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
        deployment_id,
        environment,
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
            status_handler.set_event_duration();
            status_handler.set_last_event_epoch(); // Reset the event duration timer for the next event
            status_handler.set_error_text(&error_text);
            status_handler.send_event(&handler).await;
            status_handler.send_deployment(&handler).await;
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
                                post_webhook(
                                    url,
                                    &format!(
                                        "Drift has occurred for {} in {}",
                                        deployment_id, environment
                                    ),
                                )
                                .await?;
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
                plan_raw_json_key,
                environment: environment.clone(),
                change_type: command.to_string(),
            };
            match insert_infra_change_record(&handler, infra_change_record, &command_result.stdout)
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
            status_handler.set_event_duration();
            status_handler.set_last_event_epoch(); // Reset the event duration timer for the next event
            status_handler.set_error_text(&error_text);
            status_handler.send_event(&handler).await;
            status_handler.send_deployment(&handler).await;
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
                status_handler.set_last_event_epoch(); // Reset the event duration timer for the next event
                status_handler.set_error_text(&error_text);
                status_handler.send_event(&handler).await;
                status_handler.send_deployment(&handler).await;
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
        status_handler.set_event_duration();
        status_handler.set_last_event_epoch(); // Reset the event duration timer for the next event
        status_handler.send_event(&handler).await;
        status_handler.send_deployment(&handler).await;
        exit(0);
    }

    if command == "apply" || command == "destroy" {
        let cmd = command; // from payload.command
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
                status_handler.send_event(&handler).await;
                status_handler.send_deployment(&handler).await;
            }
            Err(e) => {
                println!("Error running \"terraform {}\" command: {:?}", cmd, e);
                let error_text = e.to_string();
                let status = "error".to_string();
                status_handler.set_status(status);
                status_handler.set_event_duration();
                status_handler.set_last_event_epoch(); // Reset the event duration timer for the next event
                status_handler.set_error_text(&error_text);
                status_handler.send_event(&handler).await;
                status_handler.send_deployment(&handler).await;
                status_handler.set_error_text("");
                exit(0);
            }
        }

        if command == "apply" {
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
                            panic!("Could not parse the terraform output json from stdout: {:?}\nString was:'{:?}", e, command_result.stdout.as_str());
                        }
                    };

                    status_handler.set_output(output);
                    status_handler.send_deployment(&handler).await;
                }
                Err(e) => {
                    println!("Error: {:?}", e);

                    let status = "failed_output".to_string();
                    status_handler.set_status(status);
                    status_handler.set_event_duration();
                    status_handler.set_last_event_epoch(); // Reset the event duration timer for the next event
                    status_handler.send_event(&handler).await;
                    status_handler.send_deployment(&handler).await;
                }
            }
        }
    } else if command == "plan" {
        status_handler.set_status("successful".to_string());
        status_handler.set_event_duration();
        status_handler.set_last_event_epoch(); // Reset the event duration timer for the next event
        status_handler.send_event(&handler).await;
        status_handler.send_deployment(&handler).await;
    }

    // if !dependents.is_empty() {
    //     trigger_dependent_deployments(&dependents).await; // TODO: WIP: needs to launch with replaced variables
    // }

    println!("Done!");

    Ok(())
}

async fn trigger_dependent_deployments(dependent_deployments: &Vec<Dependent>) {
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

async fn get_module(payload: &ApiInfraPayload) -> env_defs::ModuleResp {
    let track = payload.module_track.clone();
    let handler = GenericCloudHandler::default().await;
    match handler
        .get_module_version(&payload.module, &track, &payload.module_version)
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
async fn store_backend_file() {
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
        std::process::exit(1); // Exit if writing fails
    }

    println!("Terraform backend file successfully stored in backend.tf");
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

// fn cat_file(filename: &str) {
//     println!("=== File content: {} ===", filename);
//     let output = std::process::Command::new("cat")
//         .arg(filename)
//         .output()
//         .expect("Failed to execute command");

//     println!("{}", String::from_utf8_lossy(&output.stdout));
// }

fn get_env_var(key: &str) -> String {
    match env::var(key) {
        Ok(val) => val,
        Err(_) => {
            eprintln!("Environment variable {} is not set", key);
            std::process::exit(1);
        }
    }
}

async fn download_module(s3_key: &String, destination: &str) {
    println!("Downloading module from {}...", s3_key);

    let handler = GenericCloudHandler::default().await;
    let url = match get_module_download_url(&handler, s3_key).await {
        Ok(url) => url,
        Err(e) => {
            panic!("Error: {:?}", e);
        }
    };

    match env_utils::download_zip(&url, Path::new("module.zip")).await {
        Ok(_) => {
            println!("Downloaded module");
        }
        Err(e) => {
            panic!("Error: {:?}", e);
        }
    }

    match env_utils::unzip_file(Path::new("module.zip"), Path::new(destination)) {
        Ok(_) => {
            println!("Unzipped module");
        }
        Err(e) => {
            panic!("Error: {:?}", e);
        }
    }
}

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
