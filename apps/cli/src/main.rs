
use env_aws::{publish_module, list_latest, list_environments};

use clap::{App, Arg, SubCommand};

#[tokio::main]
async fn main() {
    let matches = App::new("CLI App")
        .version("0.1.0")
        .author("InfraBridge <email@example.com>")
        .about("Handles all InfraBridge CLI operations")
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
                        .about("List all latest versions of modulef to a specific environment"),
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
            SubCommand::with_name("environment")
                .about("Work with environments")
                .subcommand(
                    SubCommand::with_name("list")
                        .about("List all environments"),
                )
        )
        .get_matches();

    match matches.subcommand() {
        Some(("module", module_matches)) => {
            match module_matches.subcommand() {
                Some(("publish", run_matches)) => {
                    let file = run_matches.value_of("file").unwrap();
                    let environment = run_matches.value_of("environment").unwrap();
                    let description = run_matches.value_of("description").unwrap_or("");
                    let reference = run_matches.value_of("ref").unwrap_or("");
                    publish_module(&file.to_string(), &environment.to_string(), &description.to_string(), &reference.to_string()).await.unwrap();
                }
                Some(("list", run_matches)) => {
                    let environment = run_matches.value_of("environment").unwrap();
                    list_latest(&environment.to_string()).await.unwrap();
                }
                _ => eprintln!("Invalid subcommand for module, must be one of 'publish', 'test', or 'version'"),
            }
        }
        Some(("environment", module_matches)) => {
            match module_matches.subcommand() {
                Some(("list", run_matches)) => {
                    list_environments().await.unwrap();
                }
                _ => eprintln!("Invalid subcommand for environment, must be 'list'"),
            }
        }
        _ => eprintln!("Invalid subcommand"),
    }
}
