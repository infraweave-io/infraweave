use anyhow::Result;
use clap::{App, Arg, SubCommand};
use env_defs::{ApiInfraPayload, Dependency};
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
            SubCommand::with_name("deploy")
                .arg(
                    Arg::with_name("environment")
                        .help("Environment used when deploying, e.g. dev, prod")
                        .required(true),
                )
                .arg(
                    Arg::with_name("claim")
                        .help("Claim file to deploy, e.g. claim.yaml")
                        .required(true),
                )
                .about("Deploy a claim to a specific environment")
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
                    .publish_module(
                        &file.to_string(),
                        &environment.to_string(),
                    )
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
                cloud_handler
                    .get_module_version(&module.to_string(), &version.to_string())
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
                    .publish_policy(
                        &file.to_string(),
                        &environment.to_string(),
                    )
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
                    .get_policy_version(&policy.to_string(), &environment.to_string(), &version.to_string())
                    .await
                    .unwrap();
            }
            _ => eprintln!(
                "Invalid subcommand for policy, must be one of 'publish', 'test', or 'version'"
            ),
        },
        Some(("deploy", run_matches)) => {
            let environment_arg = run_matches.value_of("environment").unwrap();
            let environment = format!("{}/infrabridge_cli", environment_arg);
            let claim = run_matches.value_of("claim").unwrap();
            deploy_claim(cloud_handler, &environment.to_string(), &claim.to_string())
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
                let region = "eu-central-1";
                let deployment_id = run_matches.value_of("deployment_id").unwrap();
                let environment_arg = run_matches.value_of("environment").unwrap();
                let environment = format!("{}", environment_arg);
                env_aws::read_logs(deployment_id).await.unwrap();
                cloud_handler
                    .describe_deployment_id(&deployment_id.to_string(), &environment, &region)
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
                let region = "eu-central-1";
                let deployment_id = run_matches.value_of("deployment_id").unwrap();
                let environment_arg = run_matches.value_of("environment").unwrap();
                let environment = format!("infrabridge_cli/{}", environment_arg);
                cloud_handler
                    .describe_deployment_id(&deployment_id.to_string(), &environment, &region)
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
            "Invalid subcommand, must be one of 'module', 'deploy', 'environment', or 'cloud'"
        ),
    }
}

async fn deploy_claim(
    cloud_handler: Box<dyn env_common::ModuleEnvironmentHandler>,
    environment: &String,
    claim: &String,
) -> Result<(), anyhow::Error> {
    // Read claim yaml file:
    let file_content = std::fs::read_to_string(claim).expect("Failed to read claim file");

    // Parse multiple YAML documents
    let claims: Vec<serde_yaml::Value> = serde_yaml::Deserializer::from_str(&file_content)
    .map(|doc| serde_yaml::Value::deserialize(doc).expect("Failed to parse claim file"))
    .collect();

    log::info!("Deploying {} claims in file", claims.len());
    for (_, yaml) in claims.iter().enumerate() {
    let kind = yaml["kind"].as_str().unwrap().to_string();

    let command = "apply".to_string();
    let module = kind.to_lowercase();
    let name = yaml["metadata"]["name"].as_str().unwrap().to_string();
    let environment = environment.to_string();
    let deployment_id = format!("{}/{}", module, name);
    let variables_yaml = &yaml["spec"]["variables"];
    let variables: JsonValue = if variables_yaml.is_null() {
        serde_json::json!({})
    } else {
        serde_json::to_value(variables_yaml.clone()).expect("Failed to convert spec.variables YAML to JSON")
    };
    let dependencies_yaml = &yaml["spec"]["dependencies"];
    let dependencies: Vec<Dependency> = if dependencies_yaml.is_null() {
        Vec::new()
    } else {
        dependencies_yaml.clone().as_sequence().unwrap().iter().map(|d| Dependency {
                deployment_id: format!("{}/{}", 
                    d["kind"].as_str().unwrap().to_lowercase(), 
                    d["name"].as_str().unwrap()
                ),
                environment: { // use namespace if specified, otherwise use same as deployment as default
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
        }).collect()
    };
    let module_version = yaml["spec"]["moduleVersion"].as_str().unwrap().to_string();
    let annotations: JsonValue = serde_json::to_value(yaml["metadata"]["annotations"].clone())
        .expect("Failed to convert annotations YAML to JSON");

    info!("Deploying claim to environment: {}", environment);
    info!("command: {}", command);
    info!("module: {}", module);
    info!("module_version: {}", module_version);
    info!("name: {}", name);
    info!("environment: {}", environment);
    info!("variables: {}", variables);
    info!("annotations: {}",annotations ); 
    info!("dependencies: {:?}",dependencies );

    let payload = ApiInfraPayload {
        command: command.clone(),
        module: module.clone().to_lowercase(), // TODO: Only have access to kind, not the module name (which is assumed to be lowercase of module_name)
        module_version: module_version.clone(),
        name: name.clone(),
        environment: environment.clone(),
        deployment_id: deployment_id.clone(),
        variables: variables,
        annotations: annotations,
        dependencies :dependencies,
    };

    cloud_handler.mutate_infra(payload).await?;
    }

    Ok(())
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
        .describe_deployment_id(deployment_id, &environment, region)
        .await
    {
        Ok(deployment_resp) => {
            println!("Deployment exists");
            let command = "destroy".to_string();
            let module = deployment_resp.module;
            // let name = deployment_resp.name;
            let environment = deployment_resp.environment;
            let variables: JsonValue = serde_json::to_value(&deployment_resp.variables).unwrap();
            let annotations: JsonValue = serde_json::from_str("{}").unwrap();
            let dependencies= deployment_resp.dependencies;
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

            cloud_handler.mutate_infra(payload).await?;
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
