mod deployment_status_handler;
mod job_id;
mod read;

use anyhow::{anyhow, Result};
use deployment_status_handler::DeploymentStatusHandler;
use env_defs::{ApiInfraPayload, DeploymentResp, EventData};
use job_id::get_job_id;
use log::{debug, error, info};
use serde_json::{error, Value};
use std::collections::VecDeque;
use std::fs::File;
use std::process::{exit, Command};
use std::{env, path::Path};
use tokio::io::{AsyncBufReadExt, BufReader};

use convert_case::{Case, Casing};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // let module = read_module_from_file("/tmp/s3.yaml").await?;
    // let crd_manifest = generate_crd_from_module(&module)?;
    // println!("Generated CRD Manifest:\n{}", crd_manifest);

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

    cat_file(); // DEBUG ONLY Remove this line

    println!("Read deployment id from environment variable...");

    let deployment_id = &payload.deployment_id; // get_deployment_id(&cloud_handler, &payload).await;
    let environment = &payload.environment;
    let command = &payload.command;

    let error_text = "".to_string();
    let status = "initiated".to_string(); // received, initiated, completed, failed
    let job_id = match get_job_id().await {
        Ok(id) => id,
        Err(e) => {
            println!("Error: {:?}", e);
            panic!("Error getting job id");
        }
    };

    // To reduce clutter, we can use a DeploymentStatusHandler to handle the status updates
    // since we will be updating the status multiple times and only a few fields change each time
    let mut status_handler = DeploymentStatusHandler::new(
        &cloud_handler,
        &command,
        &payload.module,
        &payload.module_version,
        &status,
        &environment,
        &deployment_id,
        &error_text,
        &job_id,
        &payload.name,
        payload.variables.clone(),
    );
    status_handler.set_command(&command);
    status_handler.set_status(&status);
    status_handler.send_event().await;
    status_handler.send_deployment().await;

    let module = get_module(&cloud_handler, &payload).await;

    println!("Run command from event...");

    download_module(&cloud_handler, module).await;

    println!("Starting command...");

    match run_terraform_init_command(&deployment_id).await {
        Ok(_) => {
            println!("Terraform init successful");
        }
        Err(e) => {
            println!("Error: {:?}", e);

            let status = "failed_init".to_string();
            status_handler.set_status(&status);
            status_handler.send_event().await;
            status_handler.send_deployment().await;
            exit(0);
            // panic!("Error running terraform init");
        }
    }

    let cmd = "validate";
    match run_terraform_command(cmd, false, false).await {
        Ok(_) => {
            println!("Terraform {} successful", cmd);
        }
        Err(e) => {
            println!("Error: {:?}", e);

            let error_text = e.to_string();
            let status = "failed_validate".to_string();
            status_handler.set_status(&status);
            status_handler.set_error_text(&error_text);
            status_handler.send_event().await;
            status_handler.send_deployment().await;
            status_handler.set_error_text("");
            exit(0);
            // panic!("Error running terraform {}", cmd);
        }
    }

    let cmd = command; // from payload.command
    status_handler.set_command(&cmd);
    match run_terraform_command(cmd, true, true).await {
        Ok(_) => {
            println!("Terraform {} successful", cmd);

            let status = "successful".to_string();
            status_handler.set_status(&status);
            if cmd == "destroy" {
                status_handler.set_deleted(true);
            }
            status_handler.send_event().await;
            status_handler.send_deployment().await;
        }
        Err(e) => {
            println!("Error: {:?}", e);
            // panic!("Error running terraform {}", cmd);

            let error_text = e.to_string();
            let status = "error".to_string();
            status_handler.set_status(&status);
            status_handler.set_error_text(&error_text);
            status_handler.send_event().await;
            status_handler.send_deployment().await;
            status_handler.set_error_text("");
            exit(0);
        }
    }
    // println!("Posting status...");
    // let command = "apply".to_string();
    // let error_text = "".to_string();
    // let job_id = "get_id_from_env".to_string(); // e.g. ECS task arn
    // let status = "finished".to_string(); // received, initiated, completed, failed

    // put_event(
    //     &cloud_handler,
    //     command,
    //     &payload.module,
    //     &status,
    //     &deployment_id,
    //     error_text,
    //     job_id,
    //     serde_json::Value::Null,
    //     &payload.name,
    // )
    // .await;

    // put_deployment(
    //     &cloud_handler,
    //     &deployment_id,
    //     &status,
    //     &payload.module,
    //     &payload.module_version,
    //     payload.variables.clone(),
    // )
    // .await;

    println!("Done!");

    Ok(())
}

async fn get_module(
    cloud_handler: &Box<dyn env_common::ModuleEnvironmentHandler>,
    payload: &ApiInfraPayload,
) -> env_defs::ModuleResp {
    match cloud_handler
        .get_module_version(&payload.module, &payload.module_version)
        .await
    {
        Ok(module) => {
            println!("Module exists: {:?}", module);
            module
        }
        Err(e) => {
            println!("Module does not exist: {:?}", e);
            panic!("Module does not exist");
        }
    }
}

async fn get_deployment_id(
    cloud_handler: &Box<dyn env_common::ModuleEnvironmentHandler>,
    payload: &ApiInfraPayload,
) -> String {
    let region = env::var("REGION").unwrap_or("eu-central-1".to_string());
    let environment = "replace".to_string();
    match cloud_handler
        .describe_deployment_id(&payload.deployment_id, &environment, &region)
        .await
    {
        Ok(deployment) => {
            println!("Deployment exists: {:?}", deployment);
            payload.deployment_id.clone()
        }
        Err(e) => {
            println!("Deployment does not exist: {:?}", e);
            // let new_deployment_id = format!(
            //     "{}-{}-{}",
            //     payload.module,
            //     payload.name,
            //     nanoid::nanoid!(6, &nanoid::alphabet::SAFE)
            // );
            // new_deployment_id
            payload.deployment_id.clone()
        }
    }
}

// async fn put_event(
//     cloud_handler: &Box<dyn env_common::ModuleEnvironmentHandler>,
//     command: String,
//     module: &String,
//     status: &String,
//     deployment_id: &String,
//     error_text: String,
//     job_id: String,
//     metadata: serde_json::Value,
//     name: &String,
// ) {
//     let epoch = std::time::UNIX_EPOCH.elapsed().unwrap().as_millis();
//     let event = EventData {
//         event: command.clone(),
//         epoch: epoch,
//         status: status.clone(),
//         module: module.clone(),
//         deployment_id: deployment_id.clone(),
//         error_text: error_text,
//         id: format!(
//             "{}-{}-{}-{}-{}",
//             module.to_string(),
//             deployment_id,
//             epoch,
//             command,
//             status
//         ),
//         job_id: job_id,
//         metadata: metadata,
//         name: name.clone(),
//         timestamp: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
//     };

//     match cloud_handler.insert_event(event).await {
//         Ok(_) => {
//             println!("Event inserted");
//         }
//         Err(e) => {
//             println!("Error: {:?}", e);
//             panic!("Error inserting event");
//         }
//     }
// }

// async fn put_deployment(
//     cloud_handler: &Box<dyn env_common::ModuleEnvironmentHandler>,
//     deployment_id: &String,
//     status: &String,
//     module: &String,
//     module_version: &String,
//     variables: serde_json::Value,
// ) {
//     let epoch = std::time::UNIX_EPOCH.elapsed().unwrap().as_millis();
//     let deployment = DeploymentResp {
//         epoch: epoch,
//         deployment_id: deployment_id.clone(),
//         status: status.clone(),
//         environment: env::var("ENVIRONMENT").unwrap(),
//         module: module.clone(),
//         module_version: module_version.clone(),
//         variables: variables.clone(),
//     };

//     match cloud_handler.set_deployment(deployment).await {
//         Ok(_) => {
//             println!("Deployment inserted");
//         }
//         Err(e) => {
//             println!("Error: {:?}", e);
//             panic!("Error inserting deployment");
//         }
//     }
// }

fn get_payload() -> ApiInfraPayload {
    let payload = env::var("PAYLOAD").unwrap();
    let payload_json: Value = match serde_json::from_str(&payload) {
        Ok(json) => json,
        Err(e) => {
            eprintln!("Failed to parse PAYLOAD as JSON: {:?}", e);
            std::process::exit(1); // Exit if parsing fails
        }
    };
    ApiInfraPayload {
        command: payload_json["command"].as_str().unwrap().to_string(),
        module: payload_json["module"].as_str().unwrap().to_string(),
        module_version: payload_json["module_version"].as_str().unwrap().to_string(),
        name: payload_json["name"].as_str().unwrap().to_string(),
        environment: payload_json["environment"].as_str().unwrap().to_string(),
        deployment_id: payload_json["deployment_id"].as_str().unwrap().to_string(),
        variables: payload_json["variables"].clone(),
        annotations: payload_json["annotations"].clone(),
    }
}

fn store_tf_vars_json(tf_vars: &Value) {
    // Convert the keys of the JSON object to snake_case
    let variables_snake_case = convert_keys_to_snake_case(tf_vars);

    // Try to create a file and write the JSON data to it
    let tf_vars_file = match File::create("terraform.tfvars.json") {
        Ok(file) => file,
        Err(e) => {
            eprintln!("Failed to create terraform.tfvars.json: {:?}", e);
            std::process::exit(1); // Exit if file creation fails
        }
    };

    // Write the JSON data to the file
    if let Err(e) = serde_json::to_writer_pretty(tf_vars_file, &variables_snake_case) {
        eprintln!("Failed to write JSON to terraform.tfvars.json: {:?}", e);
        std::process::exit(1); // Exit if writing fails
    }

    println!("Terraform variables successfully stored in terraform.tfvars.json");
}

// fn convert_keys_to_snake_case<V>(variables: HashMap<String, V>) -> HashMap<String, V> {
//     variables
//         .into_iter()
//         .map(|(k, v)| (k.to_case(Case::Snake), v))
//         .collect()
// }

fn convert_keys_to_snake_case(value: &Value) -> Value {
    match value {
        Value::Object(map) => {
            let mut new_map = tera::Map::new();
            for (key, value) in map {
                let new_key = key.to_case(Case::Snake);
                new_map.insert(new_key, value.clone());
            }
            Value::Object(new_map)
        }
        // Value::Array(array) => {
        //     let new_array = array.iter().map(convert_keys_to_snake_case).collect();
        //     Value::Array(new_array)
        // }
        _ => value.clone(),
    }
}

fn print_all_environment_variables() {
    for (key, value) in env::vars() {
        println!("{}: {}", key, value);
    }
}

fn cat_file() {
    let output = Command::new("cat")
        .arg("terraform.tfvars.json")
        .output()
        .expect("Failed to execute command");

    println!("{}", String::from_utf8_lossy(&output.stdout));
}

async fn run_terraform_init_command(deployment_id: &String) -> Result<(), anyhow::Error> {
    println!("Running terraform init command");

    let tf_bucket = get_env_var("TF_BUCKET");
    let environment = get_env_var("ENVIRONMENT");
    let region = get_env_var("REGION");
    let key = format!(
        "{}/{}/{}/terraform.tfstate",
        environment, region, deployment_id
    );

    let dynamodb_table = get_env_var("TF_DYNAMODB_TABLE");

    let init_output = Command::new("terraform")
        .arg("init")
        .arg("-no-color")
        .arg("-input=false")
        .arg(format!("-backend-config=bucket={}", tf_bucket))
        .arg(format!("-backend-config=key={}", key))
        .arg(format!("-backend-config=region={}", region))
        .arg(format!("-backend-config=dynamodb_table={}", dynamodb_table))
        .current_dir(&Path::new("./"))
        .output()?;

    let init_output_str = String::from_utf8(init_output.stdout)?;

    print!("Terraform init output: {}", init_output_str);

    if !std::process::ExitStatus::success(&init_output.status) {
        let init_error_str = String::from_utf8(init_output.stderr)?;
        info!("Terraform init failed: {}", init_error_str);
        return Err(anyhow!("Terraform init failed"));
    }

    Ok(())
}

struct TerraformCommandResult {
    stdout: String,
    stderr: String,
}

async fn run_terraform_command(
    command: &str,
    auto_approve_flag: bool,
    no_input_flag: bool,
) -> Result<(TerraformCommandResult), anyhow::Error> {
    println!("Running terraform command: {}", command);

    let mut exec = tokio::process::Command::new("terraform");
    exec.arg(command)
        .arg("-no-color")
        .current_dir(&Path::new("./"))
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped()); // Capture stdout

    if no_input_flag {
        exec.arg("-input=false");
    }

    if auto_approve_flag {
        exec.arg("-auto-approve");
    }

    let mut child = exec.spawn()?; // Start the command without waiting for it to finish
                                   // Check if `stdout` was successfully captured

    let stdout = child.stdout.take().expect("Failed to capture stdout");
    let stderr = child.stderr.take().expect("Failed to capture stderr");

    let mut stdout_reader = BufReader::new(stdout).lines();
    let mut stderr_reader = BufReader::new(stderr).lines();

    let mut last_stdout_lines = VecDeque::new();
    let mut last_stderr_lines = VecDeque::new();
    const MAX_LINES: usize = 10; // Adjust as needed

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
                        if last_stdout_lines.len() > MAX_LINES {
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
                        if last_stderr_lines.len() > MAX_LINES {
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

    // if let Some(stdout) = child.stdout.take() {
    //     let reader = std::io::BufReader::new(stdout);

    //     // Stream each line of output as it's produced
    //     for line in std::io::BufRead::lines(reader) {
    //         match line {
    //             Ok(line) => println!("{}", line), // Print each line to stdout
    //             Err(e) => error!("Error reading line: {}", e),
    //         }
    //     }
    // }

    // Wait for the command to finish
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
    Ok((TerraformCommandResult {
        stdout: stdout_text,
        stderr: stderr_text,
    }))
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
    module: env_defs::ModuleResp,
) {
    println!("Downloading module...");
    println!("Module: {:?}", module);

    let url = match cloud_handler.get_module_download_url(&module.s3_key).await {
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

    match env_utils::unzip_file(&Path::new("module.zip"), &Path::new("./")) {
        Ok(_) => {
            println!("Unzipped module");
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
