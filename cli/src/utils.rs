use env_common::interface::GenericCloudHandler;
use env_common::logic::{PROJECT_ID, REGION};
use env_defs::CloudProvider;
use http_client::{http_get_deployments, is_http_mode_enabled};
use inquire::{Select, Text};
use std::collections::HashSet;

async fn fetch_all_deployment_summaries() -> anyhow::Result<Vec<(String, String)>> {
    if is_http_mode_enabled() {
        let project = PROJECT_ID
            .get()
            .ok_or_else(|| anyhow::anyhow!("--project is required in HTTP mode"))?;
        let region = REGION
            .get()
            .ok_or_else(|| anyhow::anyhow!("--region is required in HTTP mode"))?;
        let items = http_get_deployments(project, region).await?;
        Ok(items
            .into_iter()
            .filter_map(|v| {
                let env = v.get("environment")?.as_str()?.to_string();
                let dep = v.get("deployment_id")?.as_str()?.to_string();
                Some((env, dep))
            })
            .collect())
    } else {
        let handler = current_region_handler().await;
        let deployments = handler.get_all_deployments("", false).await?;
        Ok(deployments
            .into_iter()
            .map(|d| (d.environment, d.deployment_id))
            .collect())
    }
}

pub fn get_environment(environment_arg: &str) -> String {
    if !environment_arg.contains('/') {
        format!("cli/{}", environment_arg)
    } else {
        environment_arg.to_string()
    }
}

pub async fn current_region_handler() -> GenericCloudHandler {
    // GenericCloudHandler::default() will automatically check for HTTP mode
    // and skip AWS SDK initialization if enabled
    GenericCloudHandler::default().await
}

pub async fn get_available_environments() -> anyhow::Result<Vec<String>> {
    let summaries = fetch_all_deployment_summaries().await?;
    let mut environments: Vec<String> = summaries
        .into_iter()
        .map(|(env, _)| env)
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();
    environments.sort();
    Ok(environments)
}

pub async fn get_available_deployments(environment: &str) -> anyhow::Result<Vec<String>> {
    let summaries = fetch_all_deployment_summaries().await?;
    let mut deployment_ids: Vec<String> = summaries
        .into_iter()
        .filter(|(env, _)| env == environment)
        .map(|(_, dep)| dep)
        .collect();
    deployment_ids.sort();
    Ok(deployment_ids)
}

pub async fn prompt_select_environment() -> anyhow::Result<String> {
    let environments = get_available_environments().await?;

    if environments.is_empty() {
        anyhow::bail!("No environments found. Please specify environment in the command instead.");
    }

    let selected = Select::new("Select an environment:", environments)
        .prompt()
        .map_err(|e| anyhow::anyhow!("Failed to select environment: {}", e))?;

    Ok(selected)
}

pub async fn prompt_select_deployment(environment: &str) -> anyhow::Result<String> {
    let deployments = get_available_deployments(environment).await?;

    if deployments.is_empty() {
        anyhow::bail!("No deployments found in environment '{}'", environment);
    }

    let selected = Select::new("Select a deployment:", deployments)
        .prompt()
        .map_err(|e| anyhow::anyhow!("Failed to select deployment: {}", e))?;

    Ok(selected)
}

pub async fn resolve_environment_id(environment_id: Option<String>) -> String {
    match environment_id {
        Some(id) => id,
        None => match prompt_select_environment().await {
            Ok(env) => env,
            Err(e) => {
                eprintln!("Error selecting environment: {}", e);
                std::process::exit(1);
            }
        },
    }
}

pub async fn resolve_environment_id_for_new_deployment(environment_id: Option<String>) -> String {
    if let Some(id) = environment_id {
        return id;
    }
    match Text::new("Environment id (namespace):")
        .with_default("default")
        .with_help_message("Used as the `cli/<namespace>` environment for this deployment")
        .prompt()
    {
        Ok(env) => env,
        Err(e) => {
            eprintln!("Error reading environment: {}", e);
            std::process::exit(1);
        }
    }
}

/// Resolve environment and deployment IDs together, filtering environments when deployment_id is provided
pub async fn resolve_environment_and_deployment(
    environment_id: Option<String>,
    deployment_id: Option<String>,
) -> (String, String) {
    match (deployment_id, environment_id) {
        // Both provided - use them directly
        (Some(dep_id), Some(env_id)) => (env_id, dep_id),

        // Deployment provided but not environment - filter environments by deployment
        (Some(dep_id), None) => {
            let env_id = match prompt_select_environment_for_deployment(&dep_id).await {
                Ok(env) => env,
                Err(e) => {
                    eprintln!("Error selecting environment: {}", e);
                    std::process::exit(1);
                }
            };
            (env_id, dep_id)
        }

        // Environment provided but not deployment - prompt for deployment in that environment
        (None, Some(env_id)) => {
            let env = get_environment(&env_id);
            let dep_id = match prompt_select_deployment(&env).await {
                Ok(dep) => dep,
                Err(e) => {
                    eprintln!("Error selecting deployment: {}", e);
                    std::process::exit(1);
                }
            };
            (env_id, dep_id)
        }

        // Neither provided - prompt for environment first, then deployment
        (None, None) => {
            let env_id = match prompt_select_environment().await {
                Ok(env) => env,
                Err(e) => {
                    eprintln!("Error selecting environment: {}", e);
                    std::process::exit(1);
                }
            };
            let env = get_environment(&env_id);
            let dep_id = match prompt_select_deployment(&env).await {
                Ok(dep) => dep,
                Err(e) => {
                    eprintln!("Error selecting deployment: {}", e);
                    std::process::exit(1);
                }
            };
            (env_id, dep_id)
        }
    }
}

async fn prompt_select_environment_for_deployment(deployment_id: &str) -> anyhow::Result<String> {
    let summaries = fetch_all_deployment_summaries().await?;
    let mut environments: Vec<String> = summaries
        .into_iter()
        .filter(|(_, dep)| dep == deployment_id)
        .map(|(env, _)| env)
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();

    if environments.is_empty() {
        anyhow::bail!(
            "No environments found containing deployment '{}'. Please check the deployment ID.",
            deployment_id
        );
    }

    environments.sort();

    let selected = Select::new("Select an environment:", environments)
        .prompt()
        .map_err(|e| anyhow::anyhow!("Failed to select environment: {}", e))?;

    Ok(selected)
}

pub async fn resolve_deployment_id(deployment_id: Option<String>, environment: &str) -> String {
    match deployment_id {
        Some(id) => id,
        None => match prompt_select_deployment(environment).await {
            Ok(dep) => dep,
            Err(e) => {
                eprintln!("Error selecting deployment: {}", e);
                std::process::exit(1);
            }
        },
    }
}
