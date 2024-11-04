use std::{collections::HashMap, thread, time::Duration, vec};

use anyhow::Result;
use clap::{App, Arg, SubCommand};
use colored::Colorize;
use env_aws::{bootstrap_environment, bootstrap_teardown_environment};
use env_common::{interface::{initialize_project_id, CloudHandler}, logic::{destroy_infra, driftcheck_infra, handler, is_deployment_plan_in_progress, precheck_module, publish_policy, publish_stack, run_claim}};
use env_defs::{DeploymentResp, ProjectData};
use prettytable::{row, Table};
use serde::Deserialize;

// Logging
use chrono::Local;
use log::{error, info, LevelFilter};

#[tokio::main]
async fn main() {
    initialize_project_id().await;

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
                            Arg::with_name("track")
                                .help("Track to publish to, e.g. dev, prod")
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
                        .about("Upload and publish a module to a specific track"),
                )
                .subcommand(
                    SubCommand::with_name("precheck")
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
                        .about("Precheck a module before publishing by testing provided examples"),
                )
                .subcommand(
                    SubCommand::with_name("list")
                        .arg(
                            Arg::with_name("track")
                                .help("Track to list to, e.g. dev, prod")
                                .required(true),
                        )
                        .about("List all latest versions of modules to a specific track"),
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
                                .about("Promote a version of a module to a new track, e.g. add 0.4.7 in dev to 0.4.7 in prod"),
                        ),
                ),
        )
        .subcommand(
            SubCommand::with_name("stack")
                .about("Handles stack operations")
                .subcommand(
                    SubCommand::with_name("publish")
                        .arg(
                            Arg::with_name("track")
                                .help("Track to publish to, e.g. dev, prod")
                                .required(true),
                        )
                        .arg(
                            Arg::with_name("file")
                                .help("File to the stack to publish, e.g. stack.yaml")
                                .required(true),
                        )
                        .arg(
                            Arg::with_name("ref")
                                .help("Metadata field for storing any type of reference, e.g. a git commit hash")
                                .required(false),
                        )
                        .arg(
                            Arg::with_name("description")
                                .help("Metadata field for storing a description of the stack, e.g. a git commit message")
                                .required(false),
                        )
                        .about("Upload and publish a stack to a specific track"),
                )
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
            SubCommand::with_name("set-project")
                .arg(
                    Arg::with_name("project_id")
                        .help("Project id to insert/update")
                        .required(true),
                )
                .arg(
                    Arg::with_name("name")
                        .help("Name of the project")
                        .required(true),
                )
                .arg(
                    Arg::with_name("description")
                        .help("Description about the project")
                        .required(true),
                )
                .about("Insert or update an existing project")
        )
        .subcommand(
            SubCommand::with_name("get-current-project")
                .about("Get current project")
        )
        .subcommand(
            SubCommand::with_name("get-all-projects")
                .about("Get all projects")
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
                .arg(
                    Arg::with_name("store-plan")
                        .long("store-plan")
                        .help("Flag to indicate if plan files should be stored")
                        .takes_value(false), // Indicates it's a flag, not expecting a value
                )
                .about("Plan a claim to a specific environment")
            )
        .subcommand(
            SubCommand::with_name("driftcheck")
                .arg(
                    Arg::with_name("environment")
                        .help("Environment used when planning, e.g. dev, prod")
                        .required(true),
                )
                .arg(
                    Arg::with_name("deployment_id")
                        .help("Deployment id to remove, e.g. s3bucket-my-s3-bucket-7FV")
                        .required(true),
                )
                .arg(
                    Arg::with_name("remediate")
                        .long("remediate")
                        .help("Flag to indicate if remediate should be performed")
                        .takes_value(false), // Indicates it's a flag, not expecting a value
                )
                .about("Check drift of a deployment in a specific environment")
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
                let track = run_matches.value_of("track").unwrap();
                match env_common::publish_module(&file.to_string(), &track.to_string())
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
            Some(("precheck", run_matches)) => {
                let file = run_matches.value_of("file").unwrap();
                let environment = run_matches.value_of("environment").unwrap();
                match precheck_module(&file.to_string(), &environment.to_string())
                    .await
                {
                    Ok(_) => {
                        info!("Module prechecked successfully");
                    }
                    Err(e) => {
                        error!("Failed during module precheck: {}", e);
                    }
                }
                // let example_claims = get_module_example_claims(&file.to_string()).unwrap();
                // let claim = run_matches.value_of("claim").unwrap();
                // run_claim(cloud_handler, &environment.to_string(), &claim.to_string(), &"plan".to_string())
                //     .await
                //     .unwrap();
            }
            Some(("list", run_matches)) => {
                let environment = run_matches.value_of("track").unwrap();
                let modules = handler().get_all_latest_module(&environment.to_string())
                    .await
                    .unwrap();
                println!(
                    "{:<20} {:<20} {:<20} {:<15} {:<10} {:<30}",
                    "Module", "ModuleName", "Version", "Track", "Ref", "Description"
                );
                for entry in &modules {
                    println!(
                        "{:<20} {:<20} {:<20} {:<15} {:<10} {:<30}",
                        entry.module,
                        entry.module_name,
                        entry.version,
                        entry.track,
                        entry.reference,
                        entry.description
                    );
                }
            }
            Some(("get", run_matches)) => {
                let module = run_matches.value_of("module").unwrap();
                let version = run_matches.value_of("version").unwrap();
                let track = "dev".to_string();
                handler().get_module_version(&module.to_string(), &track, &version.to_string())
                    .await
                    .unwrap();
            }
            _ => eprintln!(
                "Invalid subcommand for module, must be one of 'publish', 'test', or 'version'"
            ),
        },
        Some(("stack", stack_matches)) => match stack_matches.subcommand() {
            Some(("publish", run_matches)) => {
                let file = run_matches.value_of("file").unwrap();
                let track = run_matches.value_of("track").unwrap();
                match publish_stack(&file.to_string(), &track.to_string())
                    .await
                {
                    Ok(_) => {
                        info!("Stack published successfully");
                    }
                    Err(e) => {
                        error!("Failed to publish stack: {}", e);
                    }
                }
            }
            _ => eprintln!(
                "Invalid subcommand for stack, must be one of 'publish'"
            ),
        },
        Some(("policy", policy_matches)) => match policy_matches.subcommand() {
            Some(("publish", run_matches)) => {
                let file = run_matches.value_of("file").unwrap();
                let environment = run_matches.value_of("environment").unwrap();
                match publish_policy(&file.to_string(), &environment.to_string())
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
                handler().get_all_policies(&environment.to_string())
                    .await
                    .unwrap();
            }
            Some(("get", run_matches)) => {
                let policy = run_matches.value_of("policy").unwrap();
                let environment = run_matches.value_of("environment").unwrap();
                let version = run_matches.value_of("version").unwrap();
                handler().get_policy(
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
        Some(("set-project", run_matches)) => {
            let project_id = run_matches.value_of("project_id").unwrap();
            let name = run_matches.value_of("name").unwrap();
            let description = run_matches.value_of("description").unwrap();
            let project = ProjectData {
                project_id: project_id.to_string(),
                name: name.to_string(),
                description: description.to_string(),
            };
            match handler().set_project(&project).await {
                Ok(_) => {
                    info!("Project inserted");
                }
                Err(e) => {
                    error!("Failed to insert project: {}", e);
                }
            }
        }
        Some(("get-current-project", run_matches)) => {
            match handler().get_current_project().await {
                Ok(project) => {
                    println!("Project: {}", serde_json::to_string_pretty(&project).unwrap());
                }
                Err(e) => {
                    error!("Failed to insert project: {}", e);
                }
            }
        }
        Some(("get-all-projects", run_matches)) => {
            match handler().get_all_projects().await {
                Ok(projects) => {
                    for project in projects {
                        println!("Project: {}", serde_json::to_string_pretty(&project).unwrap());
                    }
                }
                Err(e) => {
                    error!("Failed to insert project: {}", e);
                }
            }
        }
        Some(("plan", run_matches)) => {
            let environment_arg = run_matches.value_of("environment").unwrap();
            let environment = get_environment(&environment_arg);
            let claim = run_matches.value_of("claim").unwrap();
            let store_plan = run_matches.is_present("store-plan");
            run_claim_file(&environment.to_string(), &claim.to_string(), &"plan".to_string(), store_plan)
                .await
                .unwrap();
        }
        Some(("driftcheck", run_matches)) => {
            let deployment_id = run_matches.value_of("deployment_id").unwrap();
            let environment_arg = run_matches.value_of("environment").unwrap();
            let environment = get_environment(&environment_arg);
            let remediate = run_matches.is_present("remediate");
            match driftcheck_infra(deployment_id, &environment, remediate).await {
                Ok(_) => {
                    info!("Successfully requested drift check");
                    Ok(())
                }
                Err(e) => {
                    Err(anyhow::anyhow!("Failed to request drift check: {}", e))
                }
            }.unwrap();
        }
        Some(("apply", run_matches)) => {
            let environment_arg = run_matches.value_of("environment").unwrap();
            let environment = get_environment(&environment_arg);
            let claim = run_matches.value_of("claim").unwrap();
            run_claim_file(&environment.to_string(), &claim.to_string(), &"apply".to_string(), false)
                .await
                .unwrap();
        }
        Some(("teardown", run_matches)) => {
            let deployment_id = run_matches.value_of("deployment_id").unwrap();
            let environment_arg = run_matches.value_of("environment").unwrap();
            let environment = get_environment(&environment_arg);
            match destroy_infra(deployment_id, &environment).await {
                Ok(_) => {
                    info!("Successfully requested destroying deployment");
                    Ok(())
                }
                Err(e) => {
                    Err(anyhow::anyhow!("Failed to request destroying deployment: {}", e))
                }
            }.unwrap();
        }
        Some(("deployments", module_matches)) => match module_matches.subcommand() {
            Some(("describe", run_matches)) => {
                let deployment_id = run_matches.value_of("deployment_id").unwrap();
                let environment_arg = run_matches.value_of("environment").unwrap();
                let environment = format!("{}", environment_arg);
                // env_aws::read_logs(job_id).await.unwrap();
                handler().get_deployment_and_dependents(&deployment_id.to_string(), &environment, false)
                    .await
                    .unwrap();
            }
            Some(("list", _run_matches)) => {
                let deployments = handler().get_all_deployments("").await.unwrap();
                println!(
                    "{:<20} {:<20} {:<20} {:<15} {:<10}",
                    "Deployment ID", "Module", "Version", "Environment", "Status"
                );
                for entry in &deployments {
                    println!(
                        "{:<20} {:<20} {:<20} {:<15} {:<10}",
                        entry.deployment_id,
                        entry.module,
                        entry.module_version,
                        entry.environment,
                        entry.status,
                    );
                }
            }
            _ => eprintln!("Invalid subcommand for environment, must be 'list'"),
        },
        Some(("cloud", run_matches)) => {
            let command = run_matches.value_of("command").unwrap();
            let local = true; //run_matches.value_of("local").unwrap() == "true";

            match command {
                "bootstrap" => {
                    bootstrap_environment(local, false)
                        .await
                        .unwrap();
                }
                "bootstrap-plan" => {
                    bootstrap_environment(local, true)
                        .await
                        .unwrap();
                }
                "bootstrap-teardown" => {
                    bootstrap_teardown_environment(local)
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

fn get_environment(environment_arg: &str) -> String {
    let environment = if !environment_arg.contains('/') {
        format!("{}/infraweave_cli", environment_arg)
    } else {
        environment_arg.to_string()
    };
    environment
}

async fn run_claim_file(
    environment: &String,
    claim: &String,
    command: &String,
    store_plan: bool,
) -> Result<(), anyhow::Error> {
    // Read claim yaml file:
    let file_content = std::fs::read_to_string(claim).expect("Failed to read claim file");

    // Parse multiple YAML documents
    let claims: Vec<serde_yaml::Value> = serde_yaml::Deserializer::from_str(&file_content)
        .map(|doc| serde_yaml::Value::deserialize(doc).unwrap_or("".into()))
        .collect();

    // job_id, deployment_id, environment
    let mut job_ids: Vec<(String, String, String)> = Vec::new();

    log::info!("Applying {} claims in file", claims.len());
    for (_, yaml) in claims.iter().enumerate() {
        let (job_id, deployment_id) = match run_claim(&yaml, &environment, &command).await {
            Ok((job_id, deployment_id)) => (job_id, deployment_id),
            Err(e) => {
                println!("Failed to run a manifest in claim {}: {}", claim, e);
                continue;
            }
        };
        job_ids.push((job_id, deployment_id, environment.clone()));
    }

    for (job_id, deployment_id, environment) in &job_ids {
        println!("Started {} job: {} in {} (job id: {})", command, deployment_id, environment, job_id);
    }

    if job_ids.is_empty() {
        println!("No jobs to run");
        return Ok(());
    }

    if command == "plan" {
        let (overview, std_output, violations) = match follow_plan(&job_ids).await {
            Ok((overview, std_output, violations)) => (overview, std_output, violations),
            Err(e) => {
                println!("Failed to follow plan: {}", e);
                return Err(e);
            }
        };
        if store_plan {
            std::fs::write(&"overview.txt", overview).expect("Failed to write plan overview file");
            println!("Plan overview written to overview.txt");

            std::fs::write(&"std_output.txt", std_output).expect("Failed to write plan std output file");
            println!("Plan std output written to std_output.txt");

            std::fs::write(&"violations.txt", violations).expect("Failed to write plan violations file");
            println!("Plan violations written to violations.txt");
        }
    }

    Ok(())
}

async fn follow_plan(job_ids: &Vec<(String, String, String)>) -> Result<(String, String, String), anyhow::Error>{
        // Keep track of statuses in a hashmap
        let mut statuses: HashMap<String, DeploymentResp> = HashMap::new();
        
        // Polling loop to check job statuses periodically until all are finished
        loop {
            let mut all_successful = true;

            for (job_id, deployment_id, environment) in job_ids {
                let (in_progress, job_id, deployment) = is_deployment_plan_in_progress(deployment_id, environment, job_id).await;
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


        for (job_id, deployment_id, environment) in job_ids {
            overview_table.add_row(row![
                format!("{}\n({})", deployment_id, environment),
                statuses.get(job_id).unwrap().status,
                statuses.get(job_id).unwrap().job_id,
                format!("{} policy violations", statuses.get(job_id).unwrap().policy_results.iter().filter(|p| p.failed).count())
            ]);

            match handler().get_change_record(environment, deployment_id, job_id, "PLAN").await {
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

        Ok((
            overview_table.to_string(),
            std_output_table.to_string(),
            violations_table.to_string()
        ))
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
