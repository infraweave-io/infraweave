use clap::{Args, Parser, Subcommand};
use cli::{commands, get_environment};
use env_common::interface::initialize_project_id_and_region;
use env_utils::setup_logging;

/// Get the default branch from the remote repository
fn get_default_branch() -> String {
    std::process::Command::new("git")
        .args(&["symbolic-ref", "refs/remotes/origin/HEAD", "--short"])
        .output()
        .ok()
        .and_then(|o| {
            if o.status.success() {
                String::from_utf8(o.stdout)
                    .ok()
                    .map(|s| s.trim().to_string())
            } else {
                None
            }
        })
        .unwrap_or_else(|| "origin/main".to_string())
}

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
    /// GitOps operations for detecting and processing manifest changes
    Gitops {
        #[command(subcommand)]
        command: GitopsCommands,
    },
    /// Get current project
    GetCurrentProject,
    /// Get all projects
    GetAllProjects,
    /// Plan a claim to a specific environment
    Plan {
        /// Environment id used when planning, e.g. cli/default
        environment_id: String,
        /// Claim file to deploy, e.g. claim.yaml
        claim: String,
        /// Flag to indicate if output files should be stored
        #[arg(long)]
        store_files: bool,
        /// Flag to plan a destroy operation
        #[arg(long)]
        destroy: bool,
        /// Do not follow the plan operation progress
        #[arg(long)]
        no_follow: bool,
    },
    /// Check drift of a deployment in a specific environment
    Driftcheck {
        /// Environment id used when checking drift, e.g. cli/default
        environment_id: String,
        /// Deployment id to check, e.g. s3bucket/my-s3-bucket
        deployment_id: String,
        /// Flag to indicate if remediate should be performed
        #[arg(long)]
        remediate: bool,
    },
    /// Apply a claim to a specific environment
    Apply {
        /// Environment id used when applying, e.g. cli/default
        environment_id: String,
        /// Claim file to apply, e.g. claim.yaml
        claim: String,
        /// Flag to indicate if output files should be stored
        #[arg(long)]
        store_files: bool,
        /// Do not follow the apply operation progress
        #[arg(long)]
        no_follow: bool,
    },
    /// Work with environments
    Environment {
        #[command(subcommand)]
        command: EnvironmentCommands,
    },
    /// Delete resources in cloud
    Destroy {
        /// Environment id where the deployment exists, e.g. cli/default
        environment_id: String,
        /// Deployment id to remove, e.g. s3bucket/my-s3-bucket
        deployment_id: String,
        /// Optional override version of module/stack used during destroy
        version: Option<String>,
        /// Flag to indicate if output files should be stored
        #[arg(long)]
        store_files: bool,
        /// Do not follow the destroy operation progress
        #[arg(long)]
        no_follow: bool,
    },
    /// Get YAML claim from a deployment
    GetClaim {
        /// Environment id of the existing deployment, e.g. cli/default
        environment_id: String,
        /// Deployment id to get claim for, e.g. s3bucket/my-s3-bucket
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
    /// List all latest versions of modules from a specific track
    List {
        /// Track to list from, e.g. dev, beta, stable
        track: String,
    },
    /// List information about specific version of a module
    Get {
        /// Module name to get, e.g. s3bucket
        module: String,
        /// Version to get, e.g. 0.1.4
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
    /// Track to publish to, e.g. dev, beta, stable
    track: String,
    /// Path to the module to publish, e.g. ./src
    path: String,
    /// Metadata field for storing any type of reference, e.g. a git commit hash
    #[arg(short, long)]
    r#ref: Option<String>,
    /// Metadata field for storing a description of the module, e.g. a git commit message
    #[arg(short, long)]
    description: Option<String>,
    /// Override version instead of using version from the module file
    #[arg(short, long)]
    version: Option<String>,
    /// Do not fail if the module version already exists
    #[arg(long)]
    no_fail_on_exist: bool,
}

#[derive(Args)]
struct ModulePrecheckArgs {
    /// Environment id to publish to, e.g. cli/default
    environment_id: String,
    /// Path to the module to precheck, e.g. ./src
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
        /// Path to the stack to preview, e.g. ./src
        path: String,
    },
    /// Upload and publish a stack to a specific track
    Publish(StackPublishArgs),
}

#[derive(Args)]
struct StackPublishArgs {
    /// Track to publish to, e.g. dev, beta, stable
    track: String,
    /// Path to the stack to publish, e.g. ./src
    path: String,
    /// Metadata field for storing any type of reference, e.g. a git commit hash
    #[arg(short, long)]
    r#ref: Option<String>,
    /// Metadata field for storing a description of the stack, e.g. a git commit message
    #[arg(short, long)]
    description: Option<String>,
    /// Override version instead of using version from the stack file
    #[arg(short, long)]
    version: Option<String>,
    /// Do not fail if the stack version already exists
    #[arg(long)]
    no_fail_on_exist: bool,
}

#[derive(Subcommand)]
enum PolicyCommands {
    /// Upload and publish a policy to a specific environment (not yet functional)
    Publish {
        /// Environment id to publish to, e.g. cli/default
        environment_id: String,
        /// Path to the policy to publish, e.g. ./src
        file: String,
        /// Metadata field for storing any type of reference, e.g. a git commit hash
        r#ref: Option<String>,
        /// Metadata field for storing a description of the policy, e.g. a git commit message
        description: Option<String>,
    },
    /// List all latest versions of policies from a specific environment
    List {
        /// Environment to list from, e.g. aws, azure
        environment_id: String,
    },
    /// List information about specific version of a policy
    Get {
        /// Policy name to get, e.g. s3bucket
        policy: String,
        /// Environment id to get from, e.g. cli/default
        environment_id: String,
        /// Version to get, e.g. 0.1.4
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
enum GitopsCommands {
    /// Detect changed manifests between two git references
    /// In GitHub Actions, use ${{ github.event.before }} and ${{ github.event.after }}
    /// For local testing, defaults to HEAD~1 (before) and HEAD (after)
    Diff {
        /// Git reference to compare from (e.g., commit SHA, branch, or HEAD~1 for local testing)
        /// In GitHub Actions: use ${{ github.event.before }}
        #[arg(long)]
        before: Option<String>,
        /// Git reference to compare to (e.g., commit SHA, branch, or HEAD for local testing)  
        /// In GitHub Actions: use ${{ github.event.after }}
        #[arg(long)]
        after: Option<String>,
    },
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
        /// Environment id where the deployment exists, e.g. cli/default
        environment_id: String,
        /// Deployment id to describe, e.g. s3bucket/my-s3-bucket
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
                environment_id,
                file,
                r#ref: _,
                description: _,
            } => {
                commands::policy::handle_publish(&file, &environment_id).await;
            }
            PolicyCommands::List { environment_id } => {
                commands::policy::handle_list(&environment_id).await;
            }
            PolicyCommands::Get {
                policy,
                environment_id,
                version,
            } => {
                commands::policy::handle_get(&policy, &environment_id, &version).await;
            }
            PolicyCommands::Version { command: _ } => {
                eprintln!("Policy version promote not yet implemented");
            }
        },
        Commands::Gitops { command } => match command {
            GitopsCommands::Diff { before, after } => {
                // Detect default branch and current branch
                let default_branch_full = get_default_branch(); // e.g., "origin/main"
                let default_branch_name = default_branch_full.trim_start_matches("origin/");

                let current_branch = std::process::Command::new("git")
                    .args(&["rev-parse", "--abbrev-ref", "HEAD"])
                    .output()
                    .ok()
                    .and_then(|o| {
                        if o.status.success() {
                            String::from_utf8(o.stdout)
                                .ok()
                                .map(|s| s.trim().to_string())
                        } else {
                            None
                        }
                    })
                    .unwrap_or_else(|| "HEAD".to_string());

                let is_default_branch = current_branch == default_branch_name;

                // Set defaults based on branch:
                // - On default branch: compare HEAD~1 to HEAD (what just changed)
                // - On feature branch: compare origin/main to HEAD (all changes vs main)
                let before_ref = before.as_deref().unwrap_or_else(|| {
                    if is_default_branch {
                        "HEAD~1"
                    } else {
                        Box::leak(default_branch_full.clone().into_boxed_str())
                    }
                });
                let after_ref = after.as_deref().unwrap_or("HEAD");
                commands::gitops::handle_diff(before_ref, after_ref).await;
            }
        },
        Commands::GetCurrentProject => {
            commands::project::handle_get_current().await;
        }
        Commands::GetAllProjects => {
            commands::project::handle_get_all().await;
        }
        Commands::GetClaim {
            environment_id,
            deployment_id,
        } => {
            let env = get_environment(&environment_id);
            commands::deployment::handle_get_claim(&deployment_id, &env).await;
        }
        Commands::Plan {
            environment_id,
            claim,
            store_files,
            destroy,
            no_follow,
        } => {
            let env = get_environment(&environment_id);
            commands::claim::handle_plan(&env, &claim, store_files, destroy, !no_follow).await;
        }
        Commands::Driftcheck {
            environment_id,
            deployment_id,
            remediate,
        } => {
            let env = get_environment(&environment_id);
            commands::claim::handle_driftcheck(&deployment_id, &env, remediate).await;
        }
        Commands::Apply {
            environment_id,
            claim,
            store_files,
            no_follow,
        } => {
            let env = get_environment(&environment_id);
            commands::claim::handle_apply(&env, &claim, store_files, !no_follow).await;
        }
        Commands::Destroy {
            environment_id,
            deployment_id,
            version,
            store_files,
            no_follow,
        } => {
            let env = get_environment(&environment_id);
            commands::claim::handle_destroy(
                &deployment_id,
                &env,
                version.as_deref(),
                store_files,
                !no_follow,
            )
            .await;
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
                environment_id,
                deployment_id,
            } => {
                commands::deployment::handle_describe(&deployment_id, &environment_id).await;
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
