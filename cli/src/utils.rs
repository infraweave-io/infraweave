use env_common::interface::GenericCloudHandler;
use env_defs::CloudProvider;
use inquire::Select;
use std::collections::HashSet;

pub fn get_environment(environment_arg: &str) -> String {
    if !environment_arg.contains('/') {
        format!("cli/{}", environment_arg)
    } else {
        environment_arg.to_string()
    }
}

pub async fn current_region_handler() -> GenericCloudHandler {
    GenericCloudHandler::default().await
}

pub async fn get_available_environments() -> anyhow::Result<Vec<String>> {
    let handler = current_region_handler().await;
    let deployments = handler.get_all_deployments("").await?;

    let mut environments: Vec<String> = deployments
        .iter()
        .map(|d| d.environment.clone())
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();

    environments.sort();
    Ok(environments)
}

pub async fn get_available_deployments(environment: &str) -> anyhow::Result<Vec<String>> {
    let handler = current_region_handler().await;
    let deployments = handler.get_all_deployments("").await?;

    let mut deployment_ids: Vec<String> = deployments
        .iter()
        .filter(|d| d.environment == environment)
        .map(|d| d.deployment_id.clone())
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
