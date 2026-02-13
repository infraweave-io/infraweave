use clap::{Args, Parser, Subcommand};
use cli::{commands, get_environment, resolve_environment_and_deployment, resolve_environment_id};
use env_common::interface::initialize_project_id_and_region;
use env_utils::setup_logging;

/// Get the default branch from the remote repository
fn get_default_branch() -> String {
    std::process::Command::new("git")
        .args(["symbolic-ref", "refs/remotes/origin/HEAD", "--short"])
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
    /// Handles provider operations
    Provider {
        #[command(subcommand)]
        command: ProviderCommands,
    },
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
        /// Claim file to deploy, e.g. claim.yaml
        claim: String,
        /// Environment id used when planning, e.g. cli/default (optional, will prompt if not provided)
        #[arg(short, long)]
        environment_id: Option<String>,
        /// Flag to indicate if output files should be stored
        #[arg(long)]
        store_files: bool,
        /// Flag to plan a destroy operation
        #[arg(long)]
        destroy: bool,
        /// Follow the plan operation progress
        #[arg(long)]
        follow: bool,
    },
    /// Check drift of a deployment in a specific environment
    Driftcheck {
        /// Deployment id to check, e.g. s3bucket/my-s3-bucket (optional, will prompt if not provided)
        deployment_id: Option<String>,
        /// Environment id used when checking drift, e.g. cli/default (optional, will prompt if not provided)
        #[arg(short, long)]
        environment_id: Option<String>,
        /// Flag to indicate if remediate should be performed
        #[arg(long)]
        remediate: bool,
    },
    /// Apply a claim to a specific environment
    Apply {
        /// Claim file to apply, e.g. claim.yaml
        claim: String,
        /// Environment id used when applying, e.g. cli/default (optional, will prompt if not provided)
        #[arg(short, long)]
        environment_id: Option<String>,
        /// Flag to indicate if output files should be stored
        #[arg(long)]
        store_files: bool,
        /// Follow the apply operation progress
        #[arg(long)]
        follow: bool,
    },
    /// Delete resources in cloud
    Destroy {
        /// Deployment id to remove, e.g. s3bucket/my-s3-bucket (optional, will prompt if not provided)
        deployment_id: Option<String>,
        /// Environment id where the deployment exists, e.g. cli/default (optional, will prompt if not provided)
        #[arg(short, long)]
        environment_id: Option<String>,
        /// Optional override version of module/stack used during destroy
        #[arg(short, long)]
        version: Option<String>,
        /// Flag to indicate if output files should be stored
        #[arg(long)]
        store_files: bool,
        /// Follow the destroy operation progress
        #[arg(long)]
        follow: bool,
    },
    /// Get YAML claim from a deployment
    GetClaim {
        /// Deployment id to get claim for, e.g. s3bucket/my-s3-bucket (optional, will prompt if not provided)
        deployment_id: Option<String>,
        /// Environment id of the existing deployment, e.g. cli/default (optional, will prompt if not provided)
        #[arg(short, long)]
        environment_id: Option<String>,
    },
    /// Download logs for a specific job ID
    GetLogs {
        /// Job ID to download logs for
        job_id: String,
        /// Optional output file path (prints to stdout if not specified)
        #[arg(short, long)]
        output: Option<String>,
    },
    /// Work with deployments
    Deployments {
        #[command(subcommand)]
        command: DeploymentCommands,
    },
    /// Admin operations for advanced users (workspace setup, state file access)
    /// Requires elevated permissions to perform operations
    Admin {
        #[command(subcommand)]
        command: AdminCommands,
    },
    /// Launch interactive TUI for exploring modules and deployments
    Ui,
    /// Start MCP (Model Context Protocol) server for AI tool integration
    #[command(
        about = "MCP server commands for AI tools like Claude Desktop, VSCode Copilot, etc."
    )]
    Mcp {
        #[command(subcommand)]
        command: Option<McpCommands>,
    },
    /// Generate markdown documentation (hidden)
    #[command(hide = true)]
    GenerateDocs,
    /// Upgrade to the latest released version of InfraWeave
    Upgrade {
        /// Only check for available upgrades without installing
        #[arg(long)]
        check: bool,
        /// Include pre-release versions (e.g. beta, rc)
        #[arg(long)]
        prerelease: bool,
    },
}

#[derive(Subcommand)]
enum ProviderCommands {
    Publish(ProviderPublishArgs),
    List,
}

#[derive(Args)]
struct ProviderPublishArgs {
    /// Path to the provider to publish, e.g. provider.yaml
    path: String,
    /// Metadata field for storing any type of reference, e.g. a git commit hash
    #[arg(short, long)]
    r#ref: Option<String>,
    /// Metadata field for storing a description of the provider, e.g. a git commit message
    #[arg(short, long)]
    description: Option<String>,
    /// Set version instead of in the provider file, only stable version allowed e.g. "1.0.2"
    #[arg(short, long)]
    version: Option<String>,
    /// Flag to indicate if the return code should be 0 if it already exists, otherwise 1
    #[arg(long)]
    no_fail_on_exist: bool,
}

#[derive(Subcommand)]
enum McpCommands {
    /// Setup MCP server in VS Code settings
    #[command(about = "Configure MCP server in VS Code's global settings")]
    SetupVscode,
    /// Setup MCP server in Claude Desktop settings
    #[command(about = "Configure MCP server in Claude Desktop's config")]
    SetupClaude,
}

#[derive(Subcommand)]
enum ModuleCommands {
    /// Upload and publish a module to a specific track
    Publish(ModulePublishArgs),
    /// Precheck a module before publishing by testing provided examples
    Precheck(ModulePrecheckArgs),
    /// List all latest versions of modules from a specific track
    #[command(after_help = r#"Example:
```
$ infraweave module list dev
s3bucket         v0.1.4
ec2instance      v0.2.1
rdspostgres      v1.0.0
```"#)]
    List {
        /// Track to list from, e.g. dev, beta, stable
        track: String,
    },
    /// List information about specific version of a module
    #[command(after_help = r#"Example:
```
$ infraweave module get s3bucket 0.1.4
Name: s3bucket
Version: 0.1.4
Track: dev
Created: 2025-10-15 14:30:00
```"#)]
    Get {
        /// Module name to get, e.g. s3bucket
        module: String,
        /// Version to get, e.g. 0.1.4
        version: String,
    },
    /// List all versions of a specific module on a track
    #[command(after_help = r#"Example:
```
$ infraweave module versions s3bucket dev
VERSION    STATUS       CREATED
v0.1.4     Active       2025-10-15 14:30:00
v0.1.3     DEPRECATED   2025-10-12 10:20:00
v0.1.2     Active       2025-10-10 09:15:00
```"#)]
    Versions {
        /// Module name, e.g. s3bucket
        module: String,
        /// Track to list from, e.g. dev, beta, stable
        track: String,
    },
    /// Configure versions for a module
    Version {
        #[command(subcommand)]
        command: ModuleVersionCommands,
    },
    /// Deprecate a specific version of a module
    Deprecate {
        /// Module name to deprecate, e.g. s3bucket
        module: String,
        /// Track of the module, e.g. dev, beta, stable
        track: String,
        /// Version to deprecate, e.g. 0.1.4
        version: String,
        /// Optional message explaining why the module version is deprecated
        #[arg(short, long)]
        message: Option<String>,
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
    /// Environment id to publish to, e.g. cli/default (optional, will prompt if not provided)
    environment_id: Option<String>,
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
    /// List all latest versions of stacks from a specific track
    #[command(after_help = r#"Example:
```
$ infraweave stack list dev
bucketcollection  v0.1.0
networkstack      v0.2.5
```"#)]
    List {
        /// Track to list from, e.g. dev, beta, stable
        track: String,
    },
    /// List information about specific version of a stack
    #[command(after_help = r#"Example:
```
$ infraweave stack get bucketcollection 0.1.0
Name: bucketcollection
Version: 0.1.0
Track: dev
Created: 2025-10-15 14:30:00
```"#)]
    Get {
        /// Stack name to get, e.g. bucketcollection
        stack: String,
        /// Version to get, e.g. 0.1.0
        version: String,
    },
    /// List all versions of a specific stack on a track
    #[command(after_help = r#"Example:
```
$ infraweave stack versions bucketcollection dev
VERSION    STATUS       CREATED
v0.1.0     Active       2025-10-15 14:30:00
v0.0.9     DEPRECATED   2025-10-12 10:20:00
```"#)]
    Versions {
        /// Stack name, e.g. bucketcollection
        stack: String,
        /// Track to list from, e.g. dev, beta, stable
        track: String,
    },
    /// Deprecate a specific version of a stack
    Deprecate {
        /// Stack name to deprecate, e.g. bucketcollection
        stack: String,
        /// Track of the stack, e.g. dev, beta, stable
        track: String,
        /// Version to deprecate, e.g. 0.1.4
        version: String,
        /// Optional message explaining why the stack version is deprecated
        #[arg(short, long)]
        message: Option<String>,
    },
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
        /// Environment id to publish to, e.g. cli/default (optional, will prompt if not provided)
        environment_id: Option<String>,
        /// Path to the policy to publish, e.g. ./src
        file: String,
        /// Metadata field for storing any type of reference, e.g. a git commit hash
        r#ref: Option<String>,
        /// Metadata field for storing a description of the policy, e.g. a git commit message
        description: Option<String>,
    },
    /// List all latest versions of policies from a specific environment
    List {
        /// Environment to list from, e.g. aws, azure (optional, will prompt if not provided)
        environment_id: Option<String>,
    },
    /// List information about specific version of a policy
    Get {
        /// Policy name to get, e.g. s3bucket
        policy: String,
        /// Environment id to get from, e.g. cli/default (optional, will prompt if not provided)
        environment_id: Option<String>,
        /// Version to get, e.g. 0.1.4
        version: String,
    },
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
enum DeploymentCommands {
    /// List all deployments for a specific environment
    List,
    /// Describe a specific deployment
    Describe {
        /// Environment id where the deployment exists, e.g. cli/default (optional, will prompt if not provided)
        environment_id: Option<String>,
        /// Deployment id to describe, e.g. s3bucket/my-s3-bucket (optional, will prompt if not provided)
        deployment_id: Option<String>,
    },
}

#[derive(Subcommand)]
enum AdminCommands {
    /// Set up a workspace for manual intervention on a specific deployment
    SetupWorkspace {
        /// Environment id of the deployment, e.g. cli/default (optional, will prompt if not provided)
        environment_id: Option<String>,
        /// Deployment id to set up workspace for, e.g. s3bucket/s3bucket-my-s3-bucket-7FV (optional, will prompt if not provided)
        deployment_id: Option<String>,
    },
    /// Download the Terraform state file for a specific deployment
    GetState {
        /// Environment id of the deployment, e.g. cli/default (optional, will prompt if not provided)
        environment_id: Option<String>,
        /// Deployment id to get state for, e.g. s3bucket/s3bucket-my-s3-bucket-7FV (optional, will prompt if not provided)
        deployment_id: Option<String>,
        /// Optional output file path (prints to stdout if not specified)
        #[arg(short, long)]
        output: Option<String>,
    },
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    // Skip initialization for documentation generation and MCP server
    // MCP uses stdio for JSON-RPC, so initialization logging would interfere
    let skip_init = matches!(cli.command, Commands::GenerateDocs)
        || matches!(cli.command, Commands::Upgrade { .. })
        || matches!(cli.command, Commands::Mcp { command: None })
        || matches!(
            cli.command,
            Commands::Mcp {
                command: Some(McpCommands::SetupVscode)
            }
        )
        || matches!(
            cli.command,
            Commands::Mcp {
                command: Some(McpCommands::SetupClaude)
            }
        );

    if !skip_init {
        setup_logging().unwrap();
        initialize_project_id_and_region().await;
    }

    match cli.command {
        Commands::Provider { command } => match command {
            ProviderCommands::Publish(args) => {
                commands::provider::handle_publish(
                    &args.path,
                    args.version.as_deref(),
                    args.no_fail_on_exist,
                )
                .await;
            }
            ProviderCommands::List => {
                commands::provider::handle_list().await;
            }
        },
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
                // Note: environment_id is defined but not currently used by handle_precheck
                // We'll prompt for it if not provided to maintain consistency, but it won't be used
                let _environment_id = resolve_environment_id(args.environment_id).await;
                commands::module::handle_precheck(&args.file).await;
            }
            ModuleCommands::List { track } => {
                commands::module::handle_list(&track).await;
            }
            ModuleCommands::Get { module, version } => {
                commands::module::handle_get(&module, &version).await;
            }
            ModuleCommands::Versions { module, track } => {
                commands::module::handle_versions(&module, &track).await;
            }
            ModuleCommands::Version { command: _ } => {
                eprintln!("Module version promote not yet implemented");
            }
            ModuleCommands::Deprecate {
                module,
                track,
                version,
                message,
            } => {
                commands::module::handle_deprecate(&module, &track, &version, message.as_deref())
                    .await;
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
            StackCommands::List { track } => {
                commands::stack::handle_list(&track).await;
            }
            StackCommands::Get { stack, version } => {
                commands::stack::handle_get(&stack, &version).await;
            }
            StackCommands::Versions { stack, track } => {
                commands::stack::handle_versions(&stack, &track).await;
            }
            StackCommands::Deprecate {
                stack,
                track,
                version,
                message,
            } => {
                commands::stack::handle_deprecate(&stack, &track, &version, message.as_deref())
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
                let environment_id = resolve_environment_id(environment_id).await;
                let env = get_environment(&environment_id);
                commands::policy::handle_publish(&file, &env).await;
            }
            PolicyCommands::List { environment_id } => {
                let environment_id = resolve_environment_id(environment_id).await;
                let env = get_environment(&environment_id);
                commands::policy::handle_list(&env).await;
            }
            PolicyCommands::Get {
                policy,
                environment_id,
                version,
            } => {
                let environment_id = resolve_environment_id(environment_id).await;
                let env = get_environment(&environment_id);
                commands::policy::handle_get(&policy, &env, &version).await;
            }
        },
        Commands::Gitops { command } => match command {
            GitopsCommands::Diff { before, after } => {
                // Detect default branch and current branch
                let default_branch_full = get_default_branch(); // e.g., "origin/main"
                let default_branch_name = default_branch_full.trim_start_matches("origin/");

                let current_branch = std::process::Command::new("git")
                    .args(["rev-parse", "--abbrev-ref", "HEAD"])
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
            let (environment_id, deployment_id) =
                resolve_environment_and_deployment(environment_id, deployment_id).await;
            let env = get_environment(&environment_id);
            commands::deployment::handle_get_claim(&deployment_id, &env).await;
        }
        Commands::GetLogs { job_id, output } => {
            commands::deployment::handle_get_logs(&job_id, output.as_deref()).await;
        }
        Commands::Plan {
            environment_id,
            claim,
            store_files,
            destroy,
            follow,
        } => {
            let environment_id = resolve_environment_id(environment_id).await;
            let env = get_environment(&environment_id);
            commands::claim::handle_plan(&env, &claim, store_files, destroy, follow).await;
        }
        Commands::Driftcheck {
            environment_id,
            deployment_id,
            remediate,
        } => {
            let (environment_id, deployment_id) =
                resolve_environment_and_deployment(environment_id, deployment_id).await;
            let env = get_environment(&environment_id);
            commands::claim::handle_driftcheck(&deployment_id, &env, remediate).await;
        }
        Commands::Apply {
            environment_id,
            claim,
            store_files,
            follow,
        } => {
            let environment_id = resolve_environment_id(environment_id).await;
            let env = get_environment(&environment_id);
            commands::claim::handle_apply(&env, &claim, store_files, follow).await;
        }
        Commands::Destroy {
            environment_id,
            deployment_id,
            version,
            store_files,
            follow,
        } => {
            let (environment_id, deployment_id) =
                resolve_environment_and_deployment(environment_id, deployment_id).await;
            let env = get_environment(&environment_id);
            commands::claim::handle_destroy(
                &deployment_id,
                &env,
                version.as_deref(),
                store_files,
                follow,
            )
            .await;
        }
        Commands::Deployments { command } => match command {
            DeploymentCommands::List => {
                commands::deployment::handle_list().await;
            }
            DeploymentCommands::Describe {
                environment_id,
                deployment_id,
            } => {
                let (environment_id, deployment_id) =
                    resolve_environment_and_deployment(environment_id, deployment_id).await;
                commands::deployment::handle_describe(&deployment_id, &environment_id).await;
            }
        },
        Commands::Admin { command } => match command {
            AdminCommands::SetupWorkspace {
                environment_id,
                deployment_id,
            } => {
                let (environment_id, deployment_id) =
                    resolve_environment_and_deployment(environment_id, deployment_id).await;
                commands::admin::handle_setup_workspace(&deployment_id, &environment_id).await;
            }
            AdminCommands::GetState {
                environment_id,
                deployment_id,
                output,
            } => {
                let (environment_id, deployment_id) =
                    resolve_environment_and_deployment(environment_id, deployment_id).await;
                commands::admin::handle_get_state(
                    &deployment_id,
                    &environment_id,
                    output.as_deref(),
                )
                .await;
            }
        },
        Commands::Ui => {
            if let Err(e) = run_tui().await {
                eprintln!("Error running TUI: {}", e);
                std::process::exit(1);
            }
        }
        Commands::Mcp { command } => {
            match command {
                Some(McpCommands::SetupVscode) => {
                    if let Err(e) = commands::mcp::setup_vscode().await {
                        eprintln!("Failed to setup VS Code: {}", e);
                        std::process::exit(1);
                    }
                }
                Some(McpCommands::SetupClaude) => {
                    if let Err(e) = commands::mcp::setup_claude().await {
                        eprintln!("Failed to setup Claude Desktop: {}", e);
                        std::process::exit(1);
                    }
                }
                None => {
                    // MCP server runs in async context and uses stdio for JSON-RPC
                    // Do NOT initialize project/region as it would log to stderr
                    if let Err(e) = commands::mcp::run_mcp_server().await {
                        eprintln!("MCP server error: {}", e);
                        std::process::exit(1);
                    }
                }
            }
        }
        Commands::GenerateDocs => {
            use clap_markdown::MarkdownOptions;

            let options = MarkdownOptions::new()
                .show_footer(true)
                .show_table_of_contents(false);

            let markdown = clap_markdown::help_markdown_custom::<Cli>(&options);

            // Adjust heading levels based on command depth
            let processed_markdown = markdown
                .lines()
                .map(|line| {
                    if line.starts_with("## `infraweave") {
                        let cmd_start = line.find('`').unwrap() + 1;
                        let cmd_end = line[cmd_start..].find('`').unwrap();
                        let cmd = &line[cmd_start..cmd_start + cmd_end];
                        let depth = cmd.matches(' ').count();
                        let heading = "#".repeat(2 + depth);
                        format!("{} `{}`", heading, cmd)
                    } else {
                        line.to_string()
                    }
                })
                .collect::<Vec<_>>()
                .join("\n");

            // Replace title and inject ToC
            let output = processed_markdown
                .replace("# Command-Line Help for `infraweave`", "# CLI Reference")
                .replace(
                    "This document contains the help content for the `infraweave` command-line program.",
                    "This document contains the command-line reference for the InfraWeave CLI."
                );

            println!("{}", output);
        }
        Commands::Upgrade { check, prerelease } => {
            commands::upgrade::handle_upgrade(check, prerelease).await;
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
