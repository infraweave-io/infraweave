
use env_aws::{list_environments};

use clap::{App, Arg, SubCommand};

use anyhow::Result;
use serde_json::Value as JsonValue;

// Logging
use log::{info, error, LevelFilter};
use chrono::Local;

#[tokio::main]
async fn main() {
    let cloud = "azure";
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
        .get_matches();

    // Set up logging based on the verbosity flag
    let verbose = matches.is_present("verbose");
    if verbose {
        setup_logging().unwrap();
    }

    match matches.subcommand() {
        Some(("module", module_matches)) => {
            match module_matches.subcommand() {
                Some(("publish", run_matches)) => {
                    let file = run_matches.value_of("file").unwrap();
                    let environment = run_matches.value_of("environment").unwrap();
                    let description = run_matches.value_of("description").unwrap_or("");
                    let reference = run_matches.value_of("ref").unwrap_or("");
                    cloud_handler.publish_module(&file.to_string(), &environment.to_string(), &description.to_string(), &reference.to_string()).await.unwrap();
                }
                Some(("list", run_matches)) => {
                    let environment = run_matches.value_of("environment").unwrap();
                    cloud_handler.list_module(&environment.to_string()).await.unwrap();
                }
                Some(("get", run_matches)) => {
                    let module = run_matches.value_of("module").unwrap();
                    let version = run_matches.value_of("version").unwrap();
                    cloud_handler.get_module_version(&module.to_string(), &version.to_string()).await.unwrap();
                }
                _ => error!("Invalid subcommand for module, must be one of 'publish', 'test', or 'version'"),
            }
        }
        Some(("deploy", run_matches)) => {
            let environment = run_matches.value_of("environment").unwrap();
            let claim = run_matches.value_of("claim").unwrap();
            deploy_claim(cloud_handler, &environment.to_string(), &claim.to_string()).await.unwrap();
        }
        Some(("environment", module_matches)) => {
            match module_matches.subcommand() {
                Some(("list", _run_matches)) => {
                    list_environments().await.unwrap();
                }
                _ => error!("Invalid subcommand for environment, must be 'list'"),
            }
        }
        _ => error!("Invalid subcommand, "),
    }
}

async fn deploy_claim(cloud_handler: Box<dyn env_common::ModuleEnvironmentHandler>, environment: &String, claim: &String) -> Result<(), anyhow::Error>{

    // Read claim yaml file:
    let file = std::fs::read_to_string(claim).expect("Failed to read claim file");

    let yaml: serde_yaml::Value = serde_yaml::from_str(&file).expect("Failed to parse claim file");

    let kind = yaml["kind"].as_str().unwrap().to_string();

    let event = "apply".to_string();
    let module = kind.to_lowercase();
    let name = yaml["metadata"]["name"].as_str().unwrap().to_string();
    let environment = environment.to_string();
    let deployment_id = "deployment_id".to_string();
    let spec: JsonValue = serde_json::to_value(yaml["spec"].clone()).expect("Failed to convert spec YAML to JSON");
    let annotations: JsonValue = serde_json::to_value(yaml["metadata"]["annotations"].clone()).expect("Failed to convert annotations YAML to JSON");

    info!("Deploying claim to environment: {}", environment);
    info!("event: {}", event);
    info!("module: {}", module);
    info!("name: {}", name);
    info!("environment: {}", environment);
    info!("spec: {}", spec);
    info!("annotations: {}", annotations);

    cloud_handler.mutate_infra(event, module, name, environment, deployment_id, spec, annotations).await?;

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