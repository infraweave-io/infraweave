use std::vec;

use clap::{App, Arg, SubCommand};
use cli::{current_region_handler, get_environment, run_claim_file};
use env_common::{
    errors::ModuleError,
    interface::initialize_project_id_and_region,
    logic::{
        destroy_infra, driftcheck_infra, get_stack_preview, precheck_module, publish_module,
        publish_policy, publish_stack,
    },
};
use env_defs::{CloudProvider, CloudProviderCommon, ExtraData, ProjectData};
use env_utils::setup_logging;

use log::{error, info};

#[tokio::main]
async fn main() {
    setup_logging().unwrap();
    initialize_project_id_and_region().await;

    let matches = App::new("CLI App")
        .version("0.1.0")
        .author("InfraWeave <opensource@infraweave.com>")
        .about("Handles all InfraWeave CLI operations")
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
                            Arg::with_name("path")
                                .help("Path to the module to publish, e.g. module.yaml")
                                .required(true),
                        )
                        .arg(
                            Arg::with_name("ref")
                                .short('r')
                                .long("ref")
                                .takes_value(true)
                                .help("Metadata field for storing any type of reference, e.g. a git commit hash")
                                .required(false),
                        )
                        .arg(
                            Arg::with_name("description")
                                .short('d')
                                .long("description")
                                .takes_value(true)
                                .help("Metadata field for storing a description of the module, e.g. a git commit message")
                                .required(false),
                        )
                        .arg(
                            Arg::with_name("version")
                                .short('v')
                                .long("version")
                                .help("Set version instead of in the module file")
                                .takes_value(true)
                                .required(false),
                        )
                        .arg(
                            Arg::with_name("no-fail-on-exist")
                                .help("Flag to indicate if the return code should be 0 if it already exists, otherwise 1")
                                .takes_value(false)
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
                    SubCommand::with_name("preview")
                        .arg(
                            Arg::with_name("path")
                                .help("Path to the stack to preview, e.g. stack.yaml")
                                .required(true),
                        )
                        .about("Preview a stack before publishing"),
                    )
                .subcommand(
                    SubCommand::with_name("publish")
                        .arg(
                            Arg::with_name("track")
                            .help("Track to publish to, e.g. dev, prod")
                            .required(true),
                        )
                        .arg(
                            Arg::with_name("path")
                                .help("Path to the stack to publish, e.g. stack.yaml")
                                .required(true),
                        )
                        .arg(
                            Arg::with_name("ref")
                                .help("Metadata field for storing any type of reference, e.g. a git commit hash")
                                .short('r')
                                .long("ref")
                                .takes_value(true)
                                .required(false),
                        )
                        .arg(
                            Arg::with_name("description")
                                .help("Metadata field for storing a description of the stack, e.g. a git commit message")
                                .short('d')
                                .long("description")
                                .takes_value(true)
                                .required(false),
                        )
                        .arg(
                            Arg::with_name("version")
                                .short('v')
                                .long("version")
                                .help("Set version instead of in the module file")
                                .takes_value(true)
                                .required(false),
                        )
                        .arg(
                            Arg::with_name("no-fail-on-exist")
                                .help("Flag to indicate if the return code should be 0 if it already exists, otherwise 1")
                                .takes_value(false)
                                .required(false),
                        )
                        // TODO: Implement no-fail-on-exist
                        // .arg( 
                        //     Arg::with_name("no-fail-on-exist")
                        //         .help("Flag to indicate if the return code should be 0 if it already exists, otherwise 1")
                        //         .takes_value(false)
                        //         .required(false),
                        // )
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
                                .help("Environment to publish to, e.g. aws, azure")
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
                                .help("Environment to list to, e.g. aws, azure")
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
                                .help("Environment to list to, e.g. aws, azure")
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
        .get_matches();

    match matches.subcommand() {
        Some(("module", module_matches)) => match module_matches.subcommand() {
            Some(("publish", run_matches)) => {
                let path = run_matches.value_of("path").expect("Path is required");
                let track = run_matches.value_of("track").expect("Track is required");
                let version = run_matches.value_of("version");
                let no_fail_on_exist = run_matches.is_present("no-fail-on-exist");
                match publish_module(&current_region_handler().await, path, track, version)
                    .await
                {
                    Ok(_) => {
                        info!("Module published successfully");
                    }
                    Err(ModuleError::ModuleVersionExists(version, error)) => {
                        if no_fail_on_exist {
                            info!("Module version {} already exists: {}, but continuing due to --no-fail-on-exist exits with success", version, error);
                        } else {
                            error!("Module already exists, exiting with error: {}", error);
                        }
                    }
                    Err(e) => {
                        error!("Failed to publish module: {}", e);
                    }
                }
            }
            Some(("precheck", run_matches)) => {
                let file = run_matches.value_of("file").unwrap();
                match precheck_module(&file.to_string())
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
                let modules = current_region_handler().await.get_all_latest_module(environment)
                    .await
                    .unwrap();
                println!(
                    "{:<20} {:<20} {:<20} {:<15} {:<10}",
                    "Module", "ModuleName", "Version", "Track", "Ref"
                );
                for entry in &modules {
                    println!(
                        "{:<20} {:<20} {:<20} {:<15} {:<10}",
                        entry.module,
                        entry.module_name,
                        entry.version,
                        entry.track,
                        entry.reference,
                    );
                }
            }
            Some(("get", run_matches)) => {
                let module = run_matches.value_of("module").unwrap();
                let version = run_matches.value_of("version").unwrap();
                let track = "dev".to_string();
                current_region_handler().await.get_module_version(module, &track, version)
                    .await
                    .unwrap();
            }
            _ => eprintln!(
                "Invalid subcommand for module, must be one of 'publish', 'test', or 'version'"
            ),
        },
        Some(("stack", stack_matches)) => match stack_matches.subcommand() {
            Some(("preview", run_matches)) => {
                let path = run_matches.value_of("path").expect("Path is required");
                match get_stack_preview(&current_region_handler().await, &path.to_string())
                    .await
                {
                    Ok(stack_module) => {
                        info!("Stack generated successfully");
                        println!("{}", stack_module);
                    }
                    Err(e) => {
                        error!("Failed to generate preview for stack: {}", e);
                    }
                }
            }
            Some(("publish", run_matches)) => {
                let path = run_matches.value_of("path").expect("Path is required");
                let track = run_matches.value_of("track").expect("Track is required");
                let version = run_matches.value_of("version");
                let no_fail_on_exist = run_matches.is_present("no-fail-on-exist");
                match publish_stack(&current_region_handler().await, path, track, version)
                    .await
                {
                    Ok(_) => {
                        info!("Stack published successfully");
                    }
                    Err(ModuleError::ModuleVersionExists(version, error)) => {
                        if no_fail_on_exist {
                            info!("Stack version {} already exists: {}, but continuing due to --no-fail-on-exist exits with success", version, error);
                        } else {
                            error!("Stack already exists, exiting with error: {}", error);
                        }
                    }
                    Err(e) => {
                        error!("Failed to publish stack: {}", e);
                    }
                }
            }
            _ => eprintln!(
                "Invalid subcommand for stack, must be one of 'preview', 'publish'"
            ),
        },
        Some(("policy", policy_matches)) => match policy_matches.subcommand() {
            Some(("publish", run_matches)) => {
                let file = run_matches.value_of("file").unwrap();
                let environment = run_matches.value_of("environment").unwrap();
                match publish_policy(&current_region_handler().await, file, environment)
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
                current_region_handler().await.get_all_policies(environment)
                    .await
                    .unwrap();
            }
            Some(("get", run_matches)) => {
                let policy = run_matches.value_of("policy").unwrap();
                let environment = run_matches.value_of("environment").unwrap();
                let version = run_matches.value_of("version").unwrap();
                current_region_handler().await.get_policy(
                        policy,
                        environment,
                        version,
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
                regions: vec![ // TODO: Take this as input
                    "eu-central-1".to_string(),
                    "us-west-2".to_string(),
                    "us-east-1".to_string(),
                ],
                region_map: serde_json::json!({
                    "eu-central-1": {
                        "git_provider": "gitlab",
                        "project_id": "123456",
                    },
                    "us-west-2": {
                        "git_provider": "gitlab",
                        "project_id": "12345699",
                    },
                    "us-east-1": {
                        "git_provider": "gitlab",
                        "project_id": "12345600",
                    }
                }),
            };
            match current_region_handler().await.set_project(&project).await {
                Ok(_) => {
                    info!("Project inserted");
                }
                Err(e) => {
                    error!("Failed to insert project: {}", e);
                }
            }
        }
        Some(("get-current-project", _run_matches)) => {
            match current_region_handler().await.get_current_project().await {
                Ok(project) => {
                    println!("Project: {}", serde_json::to_string_pretty(&project).unwrap());
                }
                Err(e) => {
                    error!("Failed to insert project: {}", e);
                }
            }
        }
        Some(("get-all-projects", _run_matches)) => {
            match current_region_handler().await.get_all_projects().await {
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
            let environment = get_environment(environment_arg);
            let claim = run_matches.value_of("claim").unwrap();
            let store_plan = run_matches.is_present("store-plan");
            run_claim_file(&environment, claim, "plan", store_plan)
                .await
                .unwrap();
        }
        Some(("driftcheck", run_matches)) => {
            let deployment_id = run_matches.value_of("deployment_id").unwrap();
            let environment_arg = run_matches.value_of("environment").unwrap();
            let environment = get_environment(environment_arg);
            let remediate = run_matches.is_present("remediate");
            match driftcheck_infra(&current_region_handler().await, deployment_id, &environment, remediate, ExtraData::None).await {
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
            let environment = get_environment(environment_arg);
            let claim = run_matches.value_of("claim").unwrap();
            run_claim_file(&environment, claim, "apply", false)
                .await
                .unwrap();
        }
        Some(("teardown", run_matches)) => {
            let deployment_id = run_matches.value_of("deployment_id").unwrap();
            let environment_arg = run_matches.value_of("environment").unwrap();
            let environment = get_environment(environment_arg);
            match destroy_infra(&current_region_handler().await, deployment_id, &environment, ExtraData::None).await {
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
                let environment = environment_arg.to_string();
                let (deployment, _) = current_region_handler().await.get_deployment_and_dependents(deployment_id, &environment, false)
                    .await
                    .unwrap();
                if deployment.is_some() {
                    let deployment = deployment.unwrap();
                    println!("Deployment: {}", serde_json::to_string_pretty(&deployment).unwrap());
                }
            }
            Some(("list", _run_matches)) => {
                let deployments = current_region_handler().await.get_all_deployments("").await.unwrap();
                println!(
                    "{:<50} {:<20} {:<20} {:<35} {:<10}",
                    "Deployment ID", "Module", "Version", "Environment", "Status"
                );
                for entry in &deployments {
                    println!(
                        "{:<50} {:<20} {:<20} {:<35} {:<10}",
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
        _ => eprintln!(
            "Invalid subcommand, must be one of 'module', 'apply', 'plan', 'environment', or 'cloud'"
        ),
    }
}
