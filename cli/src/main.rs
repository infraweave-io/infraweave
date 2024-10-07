use std::{collections::HashMap, thread, time::Duration, vec};

use anyhow::Result;
use clap::{App, Arg, SubCommand};
use colored::Colorize;
use env_common::DeploymentStatusHandler;
use env_defs::{ApiInfraPayload, Dependency, DeploymentResp, EventData};
use env_utils::{get_epoch, get_timestamp};
use prettytable::{row, Table};
use serde::Deserialize;
use serde_json::Value as JsonValue;

// Logging
use chrono::Local;
use log::{error, info, LevelFilter};

#[tokio::main]
async fn main() {
    let cloud = "aws";
    let cloud_handler: Box<dyn env_common::ModuleEnvironmentHandler> = match cloud {
        "azure" => Box::new(env_common::AzureHandler {}),
        "aws" => Box::new(env_common::AwsHandler {}),
        _ => panic!("Invalid cloud provider"),
    };

    let matches = App::new("CLI App")
        .version("0.1.0")
        .author("InfraBridge <email@example.com>")
        .about("Handles all InfraBridge CLI operations")
        // Use clap_verbosity_flag to add a verbosity flag
        .arg(
            Arg::new("verbose")
                .short('v')
                .long("verbose"),
        )
        .subcommand(
            SubCommand::with_name("module")
                .about("Handles module operations")
                .subcommand(
                    SubCommand::with_name("publish")
                        .arg(
                            Arg::with_name("environment")
                                .help("Environment to publish to, e.g. dev, prod")
                                .required(true),
                        )
                        .arg(
                            Arg::with_name("file")
                                .help("File to the module to publish, e.g. module.yaml")
                                .required(true),
                        )
                        .arg(
                            Arg::with_name("ref")
                                .help("Metadata field for storing any type of reference, e.g. a git commit hash")
                                .required(false),
                        )
                        .arg(
                            Arg::with_name("description")
                                .help("Metadata field for storing a description of the module, e.g. a git commit message")
                                .required(false),
                        )
                        .about("Upload and publish a module to a specific environment"),
                )
                .subcommand(
                    SubCommand::with_name("list")
                        .arg(
                            Arg::with_name("environment")
                                .help("Environment to list to, e.g. dev, prod")
                                .required(true),
                        )
                        .about("List all latest versions of modules to a specific environment"),
                )
                .subcommand(
                    SubCommand::with_name("get")
                        .arg(
                            Arg::with_name("module")
                                .help("Module to list to, e.g. s3bucket")
                                .required(true),
                        )
                        .arg(
                            Arg::with_name("version")
                                .help("Version to list to, e.g. 0.1.4")
                                .required(true),
                        )
                        .about("List information about specific version of a module"),
                )
                .subcommand(
                    SubCommand::with_name("version")
                        .about("Configure versions for a module")
                        .subcommand(
                            SubCommand::with_name("promote")
                                .about("Promote a version of a module to a new environment, e.g. add 0.4.7 in dev to 0.4.7 in prod"),
                        ),
                ),
        )
        .subcommand(
            SubCommand::with_name("policy")
                .about("Handles policy operations")
                .subcommand(
                    SubCommand::with_name("publish")
                        .arg(
                            Arg::with_name("environment")
                                .help("Environment to publish to, e.g. dev, prod")
                                .required(true),
                        )
                        .arg(
                            Arg::with_name("file")
                                .help("File to the policy to publish, e.g. policy.yaml")
                                .required(true),
                        )
                        .arg(
                            Arg::with_name("ref")
                                .help("Metadata field for storing any type of reference, e.g. a git commit hash")
                                .required(false),
                        )
                        .arg(
                            Arg::with_name("description")
                                .help("Metadata field for storing a description of the policy, e.g. a git commit message")
                                .required(false),
                        )
                        .about("Upload and publish a policy to a specific environment"),
                )
                .subcommand(
                    SubCommand::with_name("list")
                        .arg(
                            Arg::with_name("environment")
                                .help("Environment to list to, e.g. dev, prod")
                                .required(true),
                        )
                        .about("List all latest versions of policys to a specific environment"),
                )
                .subcommand(
                    SubCommand::with_name("get")
                        .arg(
                            Arg::with_name("policy")
                                .help("Policy to list to, e.g. s3bucket")
                                .required(true),
                        )
                        .arg(
                            Arg::with_name("environment")
                                .help("Environment to list to, e.g. dev, prod")
                                .required(true),
                        )
                        .arg(
                            Arg::with_name("version")
                                .help("Version to list to, e.g. 0.1.4")
                                .required(true),
                        )
                        .about("List information about specific version of a policy"),
                )
                .subcommand(
                    SubCommand::with_name("version")
                        .about("Configure versions for a policy")
                        .subcommand(
                            SubCommand::with_name("promote")
                                .about("Promote a version of a policy to a new environment, e.g. add 0.4.7 in dev to 0.4.7 in prod"),
                        ),
                ),
        )
        .subcommand(
            SubCommand::with_name("plan")
                .arg(
                    Arg::with_name("environment")
                        .help("Environment used when planning, e.g. dev, prod")
                        .required(true),
                )
                .arg(
                    Arg::with_name("claim")
                        .help("Claim file to deploy, e.g. claim.yaml")
                        .required(true),
                )
                .about("Plan a claim to a specific environment")
            )
        .subcommand(
            SubCommand::with_name("apply")
                .arg(
                    Arg::with_name("environment")
                        .help("Environment used when applying, e.g. dev, prod")
                        .required(true),
                )
                .arg(
                    Arg::with_name("claim")
                        .help("Claim file to apply, e.g. claim.yaml")
                        .required(true),
                )
                .about("Apply a claim to a specific environment")
            )
        .subcommand(
            SubCommand::with_name("environment")
                .about("Work with environments")
                .subcommand(
                    SubCommand::with_name("list")
                        .about("List all environments"),
                ),
        )
        .subcommand(
            SubCommand::with_name("teardown")
                .about("Work with environments")
                .arg(
                    Arg::with_name("environment")
                        .help("Environment used when deploying, e.g. dev, prod")
                        .required(true),
                )
                .arg(
                    Arg::with_name("deployment_id")
                        .help("Deployment id to remove, e.g. s3bucket-my-s3-bucket-7FV")
                        .required(true),
                )
                .about("Delete resources in cloud"),
        )
        .subcommand(
            SubCommand::with_name("deployments")
                .about("Work with deployments")
                .subcommand(
                    SubCommand::with_name("list")
                        .about("List all deployments for a specific environment"),
                )
                .subcommand(
                    SubCommand::with_name("describe")
                        .arg(
                            Arg::with_name("environment")
                                .help("Environment used when deploying, e.g. dev, prod")
                                .required(true),
                        )
                        .arg(
                            Arg::with_name("deployment_id")
                                .help("Deployment id to describe, e.g. s3bucket-my-s3-bucket-7FV")
                                .required(true),
                        )
                        .about("Describe a specific deployment"),
                ),
        )
        .subcommand(
            SubCommand::with_name("resources")
                .about("Work with resources")
                .subcommand(
                    SubCommand::with_name("list")
                        .arg(
                            Arg::with_name("environment")
                                .help("Environment to list resources for, e.g. dev, prod")
                                .required(true),
                        )
                        .about("List all resources for a specific environment"),
                )
                .subcommand(
                    SubCommand::with_name("describe")
                        .arg(
                            Arg::with_name("deployment_id")
                                .help("Deployment id to describe, e.g. s3bucket-my-s3-bucket-7FV")
                                .required(true),
                        )
                        .about("Describe a specific deployment"),
                ),
        )
        .subcommand(
            SubCommand::with_name("cloud")
                .about("Bootstrap environment")
                .arg(
                    Arg::with_name("command")
                        .help("Command to run, valid options are 'bootstrap', 'bootstrap-plan' or 'bootstrap-teardown'")
                        .required(true),
                )
                .arg(
                    Arg::with_name("local")
                        .help("Run terraform locally instead of inside a docker container (default is true)")
                        .required(false),
                )
        )
        .get_matches();

    // Set up logging based on the verbosity flag
    let verbose = matches.is_present("verbose");
    if verbose {
        setup_logging().unwrap();
    }

    match matches.subcommand() {
        Some(("module", module_matches)) => match module_matches.subcommand() {
            Some(("publish", run_matches)) => {
                let file = run_matches.value_of("file").unwrap();
                let environment = run_matches.value_of("environment").unwrap();
                match cloud_handler
                    .publish_module(&file.to_string(), &environment.to_string())
                    .await
                {
                    Ok(_) => {
                        info!("Module published successfully");
                    }
                    Err(e) => {
                        error!("Failed to publish module: {}", e);
                    }
                }
            }
            Some(("list", run_matches)) => {
                let environment = run_matches.value_of("environment").unwrap();
                cloud_handler
                    .list_module(&environment.to_string())
                    .await
                    .unwrap();
            }
            Some(("get", run_matches)) => {
                let module = run_matches.value_of("module").unwrap();
                let version = run_matches.value_of("version").unwrap();
                let environment = "dev".to_string();
                cloud_handler
                    .get_module_version(&module.to_string(), &environment, &version.to_string())
                    .await
                    .unwrap();
            }
            _ => eprintln!(
                "Invalid subcommand for module, must be one of 'publish', 'test', or 'version'"
            ),
        },
        Some(("policy", policy_matches)) => match policy_matches.subcommand() {
            Some(("publish", run_matches)) => {
                let file = run_matches.value_of("file").unwrap();
                let environment = run_matches.value_of("environment").unwrap();
                match cloud_handler
                    .publish_policy(&file.to_string(), &environment.to_string())
                    .await
                {
                    Ok(_) => {
                        info!("Policy published successfully");
                    }
                    Err(e) => {
                        error!("Failed to publish policy: {}", e);
                    }
                }
            }
            Some(("list", run_matches)) => {
                let environment = run_matches.value_of("environment").unwrap();
                cloud_handler
                    .list_policy(&environment.to_string())
                    .await
                    .unwrap();
            }
            Some(("get", run_matches)) => {
                let policy = run_matches.value_of("policy").unwrap();
                let environment = run_matches.value_of("environment").unwrap();
                let version = run_matches.value_of("version").unwrap();
                cloud_handler
                    .get_policy_version(
                        &policy.to_string(),
                        &environment.to_string(),
                        &version.to_string(),
                    )
                    .await
                    .unwrap();
            }
            _ => eprintln!(
                "Invalid subcommand for policy, must be one of 'publish', 'test', or 'version'"
            ),
        },
        Some(("plan", run_matches)) => {
            let environment_arg = run_matches.value_of("environment").unwrap();
            let environment = format!("{}/infrabridge_cli", environment_arg);
            let claim = run_matches.value_of("claim").unwrap();
            run_claim(cloud_handler, &environment.to_string(), &claim.to_string(), &"plan".to_string())
                .await
                .unwrap();
        }
        Some(("apply", run_matches)) => {
            let environment_arg = run_matches.value_of("environment").unwrap();
            let environment = format!("{}/infrabridge_cli", environment_arg);
            let claim = run_matches.value_of("claim").unwrap();
            run_claim(cloud_handler, &environment.to_string(), &claim.to_string(), &"apply".to_string())
                .await
                .unwrap();
        }
        Some(("teardown", run_matches)) => {
            let deployment_id = run_matches.value_of("deployment_id").unwrap();
            let environment_arg = run_matches.value_of("environment").unwrap();
            let environment = if !environment_arg.contains('/') {
                format!("{}/infrabridge_cli", environment_arg)
            } else {
                environment_arg.to_string()
            };
            teardown_deployment_id(
                cloud_handler,
                &deployment_id.to_string(),
                &environment.to_string(),
            )
            .await
            .unwrap();
        }
        Some(("environment", module_matches)) => match module_matches.subcommand() {
            Some(("list", _run_matches)) => {
                cloud_handler.list_environments().await.unwrap();
            }
            _ => eprintln!("Invalid subcommand for environment, must be 'describe' or 'list'"),
        },
        Some(("deployments", module_matches)) => match module_matches.subcommand() {
            Some(("describe", run_matches)) => {
                let deployment_id = run_matches.value_of("deployment_id").unwrap();
                let environment_arg = run_matches.value_of("environment").unwrap();
                let environment = format!("{}", environment_arg);
                // env_aws::read_logs(job_id).await.unwrap();
                cloud_handler
                    .describe_deployment_id(&deployment_id.to_string(), &environment)
                    .await
                    .unwrap();
            }
            Some(("list", _run_matches)) => {
                let environment = "dev";
                let region = "eu-central-1";
                cloud_handler.list_deployments().await.unwrap();
            }
            _ => eprintln!("Invalid subcommand for environment, must be 'list'"),
        },
        Some(("resources", module_matches)) => match module_matches.subcommand() {
            Some(("describe", run_matches)) => {
                let deployment_id = run_matches.value_of("deployment_id").unwrap();
                let environment_arg = run_matches.value_of("environment").unwrap();
                let environment = format!("infrabridge_cli/{}", environment_arg);
                cloud_handler
                    .describe_deployment_id(&deployment_id, &environment)
                    .await
                    .unwrap();
            }
            Some(("list", _run_matches)) => {
                let environment = "dev";
                let region = "eu-central-1";
                cloud_handler.list_resources(region).await.unwrap();
            }
            _ => eprintln!("Invalid subcommand for environment, must be 'list'"),
        },
        Some(("cloud", run_matches)) => {
            let command = run_matches.value_of("command").unwrap();
            let local = true; //run_matches.value_of("local").unwrap() == "true";

            match command {
                "bootstrap" => {
                    cloud_handler
                        .bootstrap_environment(local, false)
                        .await
                        .unwrap();
                }
                "bootstrap-plan" => {
                    cloud_handler
                        .bootstrap_environment(local, true)
                        .await
                        .unwrap();
                }
                "bootstrap-teardown" => {
                    cloud_handler
                        .bootstrap_teardown_environment(local)
                        .await
                        .unwrap();
                }
                _ => {
                    eprintln!(
                        "Invalid command for cloud, must be 'bootstrap' or 'bootstrap-teardown'"
                    )
                }
            }
        }
        _ => eprintln!(
            "Invalid subcommand, must be one of 'module', 'apply', 'plan', 'environment', or 'cloud'"
        ),
    }
}

async fn run_claim(
    cloud_handler: Box<dyn env_common::ModuleEnvironmentHandler>,
    environment: &String,
    claim: &String,
    command: &String,
) -> Result<(), anyhow::Error> {
    // Read claim yaml file:
    let file_content = std::fs::read_to_string(claim).expect("Failed to read claim file");

    // Parse multiple YAML documents
    let claims: Vec<serde_yaml::Value> = serde_yaml::Deserializer::from_str(&file_content)
        .map(|doc| serde_yaml::Value::deserialize(doc).expect("Failed to parse claim file"))
        .collect();

    // job_id, deployment_id, environment
    let mut job_ids: Vec<(String, String, String)> = Vec::new();

    log::info!("Applying {} claims in file", claims.len());
    for (_, yaml) in claims.iter().enumerate() {
        let kind = yaml["kind"].as_str().unwrap().to_string();

        let module = kind.to_lowercase();
        let name = yaml["metadata"]["name"].as_str().unwrap().to_string();
        let environment = environment.to_string();
        let deployment_id = format!("{}/{}", module, name);
        let variables_yaml = &yaml["spec"]["variables"];
        let variables: JsonValue = if variables_yaml.is_null() {
            serde_json::json!({})
        } else {
            serde_json::to_value(variables_yaml.clone())
                .expect("Failed to convert spec.variables YAML to JSON")
        };
        let dependencies_yaml = &yaml["spec"]["dependencies"];
        let dependencies: Vec<Dependency> = if dependencies_yaml.is_null() {
            Vec::new()
        } else {
            dependencies_yaml
                .clone()
                .as_sequence()
                .unwrap()
                .iter()
                .map(|d| Dependency {
                    deployment_id: format!(
                        "{}/{}",
                        d["kind"].as_str().unwrap().to_lowercase(),
                        d["name"].as_str().unwrap()
                    ),
                    environment: {
                        // use namespace if specified, otherwise use same as deployment as default
                        if let Some(namespace) = d.get("namespace").and_then(|n| n.as_str()) {
                            let mut env_parts = environment.split('/').collect::<Vec<&str>>();
                            if env_parts.len() == 2 {
                                env_parts[1] = namespace;
                                env_parts.join("/")
                            } else {
                                environment.clone()
                            }
                        } else {
                            environment.clone()
                        }
                    },
                })
                .collect()
        };
        let module_version = yaml["spec"]["moduleVersion"].as_str().unwrap().to_string();
        let annotations: JsonValue = serde_json::to_value(yaml["metadata"]["annotations"].clone())
            .expect("Failed to convert annotations YAML to JSON");

        info!("Applying claim to environment: {}", environment);
        info!("command: {}", command);
        info!("module: {}", module);
        info!("module_version: {}", module_version);
        info!("name: {}", name);
        info!("environment: {}", environment);
        info!("variables: {}", variables);
        info!("annotations: {}", annotations);
        info!("dependencies: {:?}", dependencies);

        let payload = ApiInfraPayload {
            command: command.clone(),
            module: module.clone().to_lowercase(), // TODO: Only have access to kind, not the module name (which is assumed to be lowercase of module_name)
            module_version: module_version.clone(),
            name: name.clone(),
            environment: environment.clone(),
            deployment_id: deployment_id.clone(),
            variables: variables,
            annotations: annotations,
            dependencies: dependencies,
        };

        let job_id = mutate_infra(&cloud_handler, &payload).await;
        job_ids.push((job_id, deployment_id, environment));
    }

    for (job_id, deployment_id, environment) in &job_ids {
        println!("Started {} job: {} in {} (job id: {})", command, deployment_id, environment, job_id);
    }

    if command == "plan" {
        // Polling loop to check job statuses periodically

        // Keep track of statuses in a hashmap
        let mut statuses: HashMap<String, DeploymentResp> = HashMap::new();

        loop {
            let mut all_successful = true;

            for (job_id, deployment_id, environment) in &job_ids {
                let (in_progress, job_id, deployment) = is_deployment_plan_in_progress(&cloud_handler, deployment_id, environment, job_id).await;
                if in_progress {
                    println!("Status of job {}: {}", job_id, if in_progress { "in progress" } else { "completed" });
                    all_successful = false;
                }

                statuses.insert(job_id.clone(), deployment.unwrap().clone());
            }

            if all_successful {
                println!("All jobs are successful!");
                break;
            }

            thread::sleep(Duration::from_secs(10));
        }

        let mut overview_table = Table::new();
        overview_table.add_row(row![
            "Deployment id\n(Environment)".purple().bold(),
            "Status".blue().bold(),
            "Job id".green().bold(),
            "Description".red().bold(),
        ]);

        let mut std_output_table = Table::new();
        std_output_table.add_row(row![
            "Deployment id\n(Environment)".purple().bold(),
            "Std output".blue().bold()
        ]);

        let mut violations_table = Table::new();
        violations_table.add_row(row![
            "Deployment id\n(Environment)".purple().bold(),
            "Policy".blue().bold(),
            "Violations".red().bold()
        ]);


        for (job_id, deployment_id, environment) in &job_ids {
            overview_table.add_row(row![
                format!("{}\n({})", deployment_id, environment),
                statuses.get(job_id).unwrap().status,
                statuses.get(job_id).unwrap().job_id,
                format!("{} policy violations", statuses.get(job_id).unwrap().policy_results.iter().filter(|p| p.failed).count())
            ]);

            match cloud_handler.get_change_record(deployment_id, environment, job_id).await {
                Ok(change_record) => {
                    println!("Change record for deployment {} in environment {}:\n{}", deployment_id, environment, change_record.plan_std_output);
                    std_output_table.add_row(row![
                        format!("{}\n({})", deployment_id, environment),
                        change_record.plan_std_output
                    ]);
                }
                Err(e) => {
                    error!("Failed to get change record: {}", e);
                }
            }

            if statuses.get(job_id).unwrap().status == "failed_policy" {
                println!("Policy validation failed for deployment {} in {}", deployment_id, environment);
                for result in statuses.get(job_id).unwrap().policy_results.iter().filter(|p| p.failed) {
                    violations_table.add_row(row![
                        format!("{}\n({})", deployment_id,environment),
                        result.policy,
                        serde_json::to_string_pretty(&result.violations).unwrap()
                    ]);
                }
                println!("Policy results: {:?}", statuses.get(job_id).unwrap().policy_results);
            }else {
                println!("Policy validation passed for deployment {:?}", statuses.get(job_id).unwrap());
            }
        }

        overview_table.printstd();
        std_output_table.printstd();
        violations_table.printstd();
    }

    Ok(())
}

async fn is_deployment_in_progress(cloud_handler: &Box<dyn env_common::ModuleEnvironmentHandler>, deployment_id: &String, environment: &String) -> (bool, String) {
    let busy_statuses = vec!["requested", "initiated"]; // TODO: use enums

    let (deployment, _) =  match cloud_handler.describe_deployment_id(deployment_id, environment).await {
        Ok((deployment_resp, dependents)) => {
            (deployment_resp, dependents)
        }
        Err(e) => {
            error!("Failed to describe deployment: {}", e);
            return (false, "".to_string());
        }
    };

    if busy_statuses.contains(&deployment.status.as_str()) {
        return (true, deployment.job_id);
    }

    (false, "".to_string())
}

async fn is_deployment_plan_in_progress(cloud_handler: &Box<dyn env_common::ModuleEnvironmentHandler>, deployment_id: &String, environment: &String, job_id: &str) -> (bool, String, Option<DeploymentResp>) {
    let busy_statuses = vec!["requested", "initiated"]; // TODO: use enums

    let deployment = match cloud_handler.describe_plan_job(deployment_id, environment, job_id).await {
        Ok(deployment_resp) => deployment_resp,
        Err(e) => {
            error!("Failed to describe deployment: {}", e);
            return (false, "".to_string(), None);
        }
    };

    let in_progress = busy_statuses.contains(&deployment.status.as_str());
    let job_id = deployment.job_id.clone();
    
    (in_progress, job_id, Some(deployment.clone()))
}

async fn mutate_infra(
    cloud_handler: &Box<dyn env_common::ModuleEnvironmentHandler>,
    payload: &ApiInfraPayload,
) -> String {

    let (in_progress, job_id) = is_deployment_in_progress(&cloud_handler, &payload.deployment_id, &payload.environment).await; // is_plan to false since only apply should be sequential (plan can be parallel)
    if in_progress {
        info!("Deployment already requested, skipping");
        println!("Deployment already requested, skipping");
        return job_id;
    }

    let job_id: String  = match cloud_handler.mutate_infra(payload.clone()).await {
        Ok(resp) => {
            info!("Request successfully submitted");
            println!("Request successfully submitted");
            let job_id = resp["job_id"].as_str().unwrap().to_string();
            job_id
        }
        Err(e) => {
            let error_text = e.to_string();
            error!("Failed to deploy claim: {}", &error_text);
            panic!("Failed to deploy claim: {}", &error_text);
        }
    };

    let mut status_handler = DeploymentStatusHandler::new(
        &cloud_handler,
        &payload.command,
        &payload.module,
        &payload.module_version,
        "requested".to_string(),
        &payload.environment,
        &payload.deployment_id,
        "",
        &job_id,
        &payload.name,
        payload.variables.clone(),
        payload.dependencies.clone(),
        serde_json::Value::Null,
        vec![],
    );
    status_handler.send_event().await;
    status_handler.send_deployment().await;

    job_id
}

async fn teardown_deployment_id(
    cloud_handler: Box<dyn env_common::ModuleEnvironmentHandler>,
    deployment_id: &String,
    environment: &String,
) -> Result<(), anyhow::Error> {
    let name = "".to_string();
    // let annotations: JsonValue = serde_json::Value::Null;

    let region = "eu-central-1";
    match cloud_handler
        .describe_deployment_id(deployment_id, &environment)
        .await
    {
        Ok((deployment_resp, dependents)) => {
            println!("Deployment exists");
            let command = "destroy".to_string();
            let module = deployment_resp.module;
            // let name = deployment_resp.name;
            let environment = deployment_resp.environment;
            let variables: JsonValue = serde_json::to_value(&deployment_resp.variables).unwrap();
            let annotations: JsonValue = serde_json::from_str("{}").unwrap();
            let dependencies = deployment_resp.dependencies;
            let module_version = deployment_resp.module_version;

            info!("Tearing down deployment: {}", deployment_id);
            info!("command: {}", command);
            // info!("module: {}", module);
            // info!("name: {}", name);
            // info!("environment: {}", environment);
            info!("variables: {}", variables);
            info!("annotations: {}", annotations);
            info!("dependencies: {:?}", dependencies);

            let payload = ApiInfraPayload {
                command: command.clone(),
                module: module.clone().to_lowercase(), // TODO: Only have access to kind, not the module name (which is assumed to be lowercase of module_name)
                module_version: module_version.clone(),
                name: name.clone(),
                environment: environment.clone(),
                deployment_id: deployment_id.clone(),
                variables: variables,
                annotations: annotations,
                dependencies: dependencies,
            };

            let job_id: String = mutate_infra(&cloud_handler, &payload).await;
        }
        Err(e) => {
            error!("Failed to describe deployment: {}", e);
        }
    }

    Ok(())
}

fn setup_logging() -> Result<(), fern::InitError> {
    let base_config = fern::Dispatch::new();

    let stdout_config = fern::Dispatch::new()
        .format(|out, message, record| {
            out.finish(format_args!(
                "{}[{}] {}: {}",
                Local::now().format("[%Y-%m-%d][%H:%M:%S]"),
                record.target(),
                record.level(),
                message
            ))
        })
        .level(LevelFilter::Debug)
        .chain(std::io::stdout());

    // let file_config = fern::Dispatch::new()
    //     .format(|out, message, record| {
    //         out.finish(format_args!(
    //             "{}[{}] {}: {}",
    //             Local::now().format("[%Y-%m-%d][%H:%M:%S]"),
    //             record.target(),
    //             record.level(),
    //             message
    //         ))
    //     })
    //     .level(LevelFilter::Info)
    //     .chain(fern::log_file("output.log")?);

    base_config
        .chain(stdout_config)
        // .chain(file_config)
        .apply()?;

    Ok(())
}
