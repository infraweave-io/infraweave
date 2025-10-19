use clap::{Args, Parser, Subcommand};
use cli::{commands, get_environment};
use env_common::interface::initialize_project_id_and_region;
use env_utils::setup_logging;

/// InfraWeave CLI - Handles all InfraWeave CLI operations
#[derive(Parser)]
#[command(name = "InfraWeave CLI")]
#[command(version = env!("APP_VERSION"))]
#[command(bin_name = "infraweave")]
#[command(author = "InfraWeave <opensource@infraweave.com>")]
#[command(about = "Handles all InfraWeave CLI operations")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Handles module operations
    Module {
        #[command(subcommand)]
        command: ModuleCommands,
    },
    /// Handles stack operations
    Stack {
        #[command(subcommand)]
        command: StackCommands,
    },
    /// Handles policy operations
    Policy {
        #[command(subcommand)]
        command: PolicyCommands,
    },
    /// Get current project
    GetCurrentProject,
    /// Get all projects
    GetAllProjects,
    /// Plan a claim to a specific environment
    Plan {
        /// Environment used when planning, e.g. dev, prod
        environment: String,
        /// Claim file to deploy, e.g. claim.yaml
        claim: String,
        /// Flag to indicate if plan files should be stored
        #[arg(long)]
        store_plan: bool,
    },
    /// Check drift of a deployment in a specific environment
    Driftcheck {
        /// Environment used when planning, e.g. dev, prod
        environment: String,
        /// Deployment id to remove, e.g. s3bucket-my-s3-bucket-7FV
        deployment_id: String,
        /// Flag to indicate if remediate should be performed
        #[arg(long)]
        remediate: bool,
    },
    /// Apply a claim to a specific environment
    Apply {
        /// Environment used when applying, e.g. dev, prod
        environment: String,
        /// Claim file to apply, e.g. claim.yaml
        claim: String,
    },
    /// Work with environments
    Environment {
        #[command(subcommand)]
        command: EnvironmentCommands,
    },
    /// Delete resources in cloud
    Destroy {
        /// Environment used when deploying, e.g. dev, prod
        environment: String,
        /// Deployment id to remove, e.g. s3bucket-my-s3-bucket-7FV
        deployment_id: String,
        /// Optional override version of module/stack used during destroy
        version: Option<String>,
    },
    /// Get YAML claim from a deployment
    GetClaim {
        /// Environment of the existing deployment, e.g. cli or playground
        environment: String,
        /// Deployment id to get claim for, e.g. s3bucket-my-s3-bucket-7FV
        deployment_id: String,
    },
    /// Work with deployments
    Deployments {
        #[command(subcommand)]
        command: DeploymentCommands,
    },
    /// Launch interactive TUI for exploring modules and deployments
    Ui,
    /// Generate markdown documentation (hidden)
    #[command(hide = true)]
    GenerateDocs,
}

#[derive(Subcommand)]
enum ModuleCommands {
    /// Upload and publish a module to a specific track
    Publish(ModulePublishArgs),
    /// Precheck a module before publishing by testing provided examples
    Precheck(ModulePrecheckArgs),
    /// List all latest versions of modules to a specific track
    List {
        /// Track to list to, e.g. dev, prod
        track: String,
    },
    /// List information about specific version of a module
    Get {
        /// Module to list to, e.g. s3bucket
        module: String,
        /// Version to list to, e.g. 0.1.4
        version: String,
    },
    /// Configure versions for a module
    Version {
        #[command(subcommand)]
        command: ModuleVersionCommands,
    },
}

#[derive(Args)]
struct ModulePublishArgs {
    /// Track to publish to, e.g. dev, prod
    track: String,
    /// Path to the module to publish, e.g. module.yaml
    path: String,
    /// Metadata field for storing any type of reference, e.g. a git commit hash
    #[arg(short, long)]
    r#ref: Option<String>,
    /// Metadata field for storing a description of the module, e.g. a git commit message
    #[arg(short, long)]
    description: Option<String>,
    /// Set version instead of in the module file
    #[arg(short, long)]
    version: Option<String>,
    /// Flag to indicate if the return code should be 0 if it already exists, otherwise 1
    #[arg(long)]
    no_fail_on_exist: bool,
}

#[derive(Args)]
struct ModulePrecheckArgs {
    /// Environment to publish to, e.g. dev, prod
    environment: String,
    /// File to the module to publish, e.g. module.yaml
    file: String,
    /// Metadata field for storing any type of reference, e.g. a git commit hash
    r#ref: Option<String>,
    /// Metadata field for storing a description of the module, e.g. a git commit message
    description: Option<String>,
}

#[derive(Subcommand)]
enum ModuleVersionCommands {
    /// Promote a version of a module to a new track, e.g. add 0.4.7 in dev to 0.4.7 in prod
    Promote,
}

#[derive(Subcommand)]
enum StackCommands {
    /// Preview a stack before publishing
    Preview {
        /// Path to the stack to preview, e.g. stack.yaml
        path: String,
    },
    /// Upload and publish a stack to a specific track
    Publish(StackPublishArgs),
}

#[derive(Args)]
struct StackPublishArgs {
    /// Track to publish to, e.g. dev, prod
    track: String,
    /// Path to the stack to publish, e.g. stack.yaml
    path: String,
    /// Metadata field for storing any type of reference, e.g. a git commit hash
    #[arg(short, long)]
    r#ref: Option<String>,
    /// Metadata field for storing a description of the stack, e.g. a git commit message
    #[arg(short, long)]
    description: Option<String>,
    /// Set version instead of in the module file
    #[arg(short, long)]
    version: Option<String>,
    /// Flag to indicate if the return code should be 0 if it already exists, otherwise 1
    #[arg(long)]
    no_fail_on_exist: bool,
}

#[derive(Subcommand)]
enum PolicyCommands {
    /// Upload and publish a policy to a specific environment
    Publish {
        /// Environment to publish to, e.g. aws, azure
        environment: String,
        /// File to the policy to publish, e.g. policy.yaml
        file: String,
        /// Metadata field for storing any type of reference, e.g. a git commit hash
        r#ref: Option<String>,
        /// Metadata field for storing a description of the policy, e.g. a git commit message
        description: Option<String>,
    },
    /// List all latest versions of policies to a specific environment
    List {
        /// Environment to list to, e.g. aws, azure
        environment: String,
    },
    /// List information about specific version of a policy
    Get {
        /// Policy to list to, e.g. s3bucket
        policy: String,
        /// Environment to list to, e.g. aws, azure
        environment: String,
        /// Version to list to, e.g. 0.1.4
        version: String,
    },
    /// Configure versions for a policy
    Version {
        #[command(subcommand)]
        command: PolicyVersionCommands,
    },
}

#[derive(Subcommand)]
enum PolicyVersionCommands {
    /// Promote a version of a policy to a new environment, e.g. add 0.4.7 in dev to 0.4.7 in prod
    Promote,
}

#[derive(Subcommand)]
enum EnvironmentCommands {
    /// List all environments
    List,
}

#[derive(Subcommand)]
enum DeploymentCommands {
    /// List all deployments for a specific environment
    List,
    /// Describe a specific deployment
    Describe {
        /// Environment used when deploying, e.g. dev, prod
        environment: String,
        /// Deployment id to describe, e.g. s3bucket-my-s3-bucket-7FV
        deployment_id: String,
    },
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    setup_logging().unwrap();
    initialize_project_id_and_region().await;

    match cli.command {
        Commands::Module { command } => match command {
            ModuleCommands::Publish(args) => {
                commands::module::handle_publish(
                    &args.path,
                    &args.track,
                    args.version.as_deref(),
                    args.no_fail_on_exist,
                )
                .await;
            }
            ModuleCommands::Precheck(args) => {
                commands::module::handle_precheck(&args.file).await;
            }
            ModuleCommands::List { track } => {
                commands::module::handle_list(&track).await;
            }
            ModuleCommands::Get { module, version } => {
                commands::module::handle_get(&module, &version).await;
            }
            ModuleCommands::Version { command: _ } => {
                eprintln!("Module version promote not yet implemented");
            }
        },
        Commands::Stack { command } => match command {
            StackCommands::Preview { path } => {
                commands::stack::handle_preview(&path).await;
            }
            StackCommands::Publish(args) => {
                commands::stack::handle_publish(
                    &args.path,
                    &args.track,
                    args.version.as_deref(),
                    args.no_fail_on_exist,
                )
                .await;
            }
        },
        Commands::Policy { command } => match command {
            PolicyCommands::Publish {
                environment,
                file,
                r#ref: _,
                description: _,
            } => {
                commands::policy::handle_publish(&file, &environment).await;
            }
            PolicyCommands::List { environment } => {
                commands::policy::handle_list(&environment).await;
            }
            PolicyCommands::Get {
                policy,
                environment,
                version,
            } => {
                commands::policy::handle_get(&policy, &environment, &version).await;
            }
            PolicyCommands::Version { command: _ } => {
                eprintln!("Policy version promote not yet implemented");
            }
        },
        Commands::GetCurrentProject => {
            commands::project::handle_get_current().await;
        }
        Commands::GetAllProjects => {
            commands::project::handle_get_all().await;
        }
        Commands::GetClaim {
            environment,
            deployment_id,
        } => {
            let env = get_environment(&environment);
            commands::deployment::handle_get_claim(&deployment_id, &env).await;
        }
        Commands::Plan {
            environment,
            claim,
            store_plan,
        } => {
            let env = get_environment(&environment);
            commands::claim::handle_plan(&env, &claim, store_plan).await;
        }
        Commands::Driftcheck {
            environment,
            deployment_id,
            remediate,
        } => {
            let env = get_environment(&environment);
            commands::claim::handle_driftcheck(&deployment_id, &env, remediate).await;
        }
        Commands::Apply { environment, claim } => {
            let env = get_environment(&environment);
            commands::claim::handle_apply(&env, &claim).await;
        }
        Commands::Destroy {
            environment,
            deployment_id,
            version,
        } => {
            let env = get_environment(&environment);
            commands::claim::handle_destroy(&deployment_id, &env, version.as_deref()).await;
        }
        Commands::Environment { command } => match command {
            EnvironmentCommands::List => {
                eprintln!("Environment list not yet implemented");
            }
        },
        Commands::Deployments { command } => match command {
            DeploymentCommands::List => {
                commands::deployment::handle_list().await;
            }
            DeploymentCommands::Describe {
                environment,
                deployment_id,
            } => {
                commands::deployment::handle_describe(&deployment_id, &environment).await;
            }
        },
        Commands::Ui => {
            if let Err(e) = run_tui().await {
                eprintln!("Error running TUI: {}", e);
                std::process::exit(1);
            }
        }
        Commands::GenerateDocs => {
            clap_markdown::print_help_markdown::<Cli>();
        }
    }
}

async fn run_tui() -> anyhow::Result<()> {
    use crossterm::{
        execute,
        terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
    };
    use ratatui::{backend::CrosstermBackend, Terminal};
    use std::io;

    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Create app and background task channel
    let mut app = cli::tui::App::new();
    let (bg_sender, mut bg_receiver) = cli::tui::background::create_channel();
    app.set_background_sender(bg_sender);

    // Main loop
    loop {
        // Process all pending background messages (non-blocking)
        cli::tui::background_tasks::process_background_messages(&mut app, &mut bg_receiver);

        // Check if we should trigger a reload after track switch
        app.check_track_switch_timeout();

        // Prepare loading state for pending actions
        if app.has_pending_action() {
            app.prepare_pending_action();
        }

        // Render the UI (will show loading screen if action is pending)
        terminal.draw(|f| cli::tui::ui::render(f, &mut app))?;

        // Process any pending actions after showing loading screen
        if app.has_pending_action() {
            app.process_pending_action().await?;
            continue; // Render the result immediately
        }

        // Handle user input events
        cli::tui::handlers::handle_events(&mut app).await?;

        if app.should_quit {
            break;
        }
    }

    // Restore terminal
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    Ok(())
}
