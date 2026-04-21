use anyhow::Result;
use http_client::{
    http_describe_deployment, http_get_deployments, http_get_logs, http_get_module_version,
    is_http_mode_enabled,
};
use log::error;

use super::{exit_on_err, exit_on_none, fetch_all_projects};
use crate::current_region_handler;
use env_defs::{CloudProvider, CloudProviderCommon, DeploymentResp, ModuleResp};

async fn fetch_deployment(
    deployment_id: &str,
    environment: &str,
) -> Result<Option<DeploymentResp>> {
    if is_http_mode_enabled() {
        let handler = current_region_handler().await;
        let value = http_describe_deployment(
            handler.get_project_id(),
            handler.get_region(),
            environment,
            deployment_id,
        )
        .await?;
        if value.is_null() {
            return Ok(None);
        }
        Ok(Some(serde_json::from_value(value)?))
    } else {
        let (dep, _) = current_region_handler()
            .await
            .get_deployment_and_dependents(deployment_id, environment, false)
            .await?;
        Ok(dep)
    }
}

async fn fetch_module_version(module: &str, track: &str, version: &str) -> Result<ModuleResp> {
    if is_http_mode_enabled() {
        Ok(http_get_module_version(track, module, version).await?)
    } else {
        current_region_handler()
            .await
            .get_module_version(module, track, version)
            .await?
            .ok_or_else(|| anyhow::anyhow!("Module {module} {track} {version} not found"))
    }
}

async fn fetch_logs(job_id: &str) -> Result<String> {
    if is_http_mode_enabled() {
        let handler = current_region_handler().await;
        Ok(http_get_logs(handler.get_project_id(), handler.get_region(), job_id).await?)
    } else {
        let logs = current_region_handler().await.read_logs(job_id).await?;
        Ok(logs
            .iter()
            .map(|l| l.message.as_str())
            .collect::<Vec<_>>()
            .join("\n"))
    }
}

async fn fetch_deployments(project: &str, region: &str) -> Result<Vec<DeploymentResp>> {
    if is_http_mode_enabled() {
        http_get_deployments(project, region)
            .await?
            .into_iter()
            .map(|v| serde_json::from_value(v).map_err(Into::into))
            .collect()
    } else {
        Ok(
            env_common::interface::GenericCloudHandler::workload(project, region)
                .await
                .get_all_deployments("", false)
                .await?,
        )
    }
}

async fn fetch_deployments_across_projects(
    filter_project: Option<&str>,
    filter_region: Option<&str>,
) -> Result<Vec<DeploymentResp>> {
    let projects = fetch_all_projects().await?;
    let mut all = Vec::new();
    for pd in projects {
        if let Some(p) = filter_project {
            if p != pd.project_id {
                continue;
            }
        }
        let regions: Vec<String> = match filter_region {
            Some(r) if pd.regions.contains(&r.to_string()) => vec![r.to_string()],
            Some(_) => vec![],
            None => pd.regions,
        };
        for r in regions {
            match fetch_deployments(&pd.project_id, &r).await {
                Ok(deps) => all.extend(deps),
                Err(e) => error!(
                    "Failed to fetch deployments for {}/{}: {}",
                    pd.project_id, r, e
                ),
            }
        }
    }
    Ok(all)
}

pub async fn handle_describe(deployment_id: &str, environment: &str) {
    let d = exit_on_none(
        exit_on_err(fetch_deployment(deployment_id, environment).await),
        &format!("Deployment not found: {}", deployment_id),
    );
    println!("Deployment: {}", serde_json::to_string_pretty(&d).unwrap());
}

pub async fn handle_list(project: Option<&str>, region: Option<&str>) {
    let all_deployments = if let (Some(p), Some(r)) = (project, region) {
        exit_on_err(fetch_deployments(p, r).await)
    } else {
        exit_on_err(fetch_deployments_across_projects(project, region).await)
    };

    println!(
        "{:<15} {:<30} {:<15} {:<50} {:<20} {:<25} {:<40}",
        "Status", "Project", "Region", "Deployment ID", "Module", "Version", "Environment",
    );
    for entry in &all_deployments {
        println!(
            "{:<15} {:<30} {:<15} {:<50} {:<20} {:<25} {:<40}",
            entry.status,
            entry.project_id,
            entry.region,
            entry.deployment_id,
            entry.module,
            format!(
                "{}{}",
                &entry.module_version.chars().take(21).collect::<String>(),
                if entry.module_version.len() > 21 {
                    "..."
                } else {
                    ""
                },
            ),
            entry.environment,
        );
    }
}

pub async fn handle_get_claim(deployment_id: &str, environment: &str) {
    let deployment = exit_on_none(
        exit_on_err(fetch_deployment(deployment_id, environment).await),
        &format!("Deployment not found: {}", deployment_id),
    );

    let module = exit_on_err(
        fetch_module_version(
            &deployment.module,
            &deployment.module_track,
            &deployment.module_version,
        )
        .await,
    );

    println!(
        "{}",
        env_utils::generate_deployment_claim(&deployment, &module)
    );
}

pub async fn handle_get_logs(job_id: &str, output_path: Option<&str>) {
    let log_content = exit_on_err(fetch_logs(job_id).await);

    if let Some(path) = output_path {
        exit_on_err(
            std::fs::write(path, &log_content)
                .map_err(|e| anyhow::anyhow!("Failed to write to {}: {}", path, e)),
        );
        println!("Logs successfully written to: {}", path);
    } else {
        println!("{}", log_content);
    }
}
