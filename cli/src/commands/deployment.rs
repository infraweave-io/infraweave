use log::error;

use crate::current_region_handler;
use env_defs::{CloudProvider, CloudProviderCommon};
use std::fs::File;
use std::io::Write;

pub async fn handle_describe(deployment_id: &str, environment: &str) {
    let (deployment, _) = current_region_handler()
        .await
        .get_deployment_and_dependents(deployment_id, environment, false)
        .await
        .unwrap();
    if let Some(deployment) = deployment {
        println!(
            "Deployment: {}",
            serde_json::to_string_pretty(&deployment).unwrap()
        );
    }
}

pub async fn handle_list() {
    let deployments = current_region_handler()
        .await
        .get_all_deployments("", false)
        .await
        .unwrap();
    println!(
        "{:<15} {:<50} {:<20} {:<25} {:<40}",
        "Status", "Deployment ID", "Module", "Version", "Environment",
    );
    for entry in &deployments {
        println!(
            "{:<15} {:<50} {:<20} {:<25} {:<40}",
            entry.status,
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
    match current_region_handler()
        .await
        .get_deployment(deployment_id, environment, false)
        .await
    {
        Ok(deployment) => {
            if let Some(deployment) = deployment {
                let module = current_region_handler()
                    .await
                    .get_module_version(
                        &deployment.module,
                        &deployment.module_track,
                        &deployment.module_version,
                    )
                    .await
                    .unwrap()
                    .unwrap();

                println!(
                    "{}",
                    env_utils::generate_deployment_claim(&deployment, &module)
                );
            } else {
                error!("Deployment not found: {}", deployment_id);
                std::process::exit(1);
            }
        }
        Err(e) => {
            error!("Failed to get claim: {}", e);
            std::process::exit(1);
        }
    }
}

pub async fn handle_get_logs(job_id: &str, output_path: Option<&str>) {
    match current_region_handler().await.read_logs(job_id).await {
        Ok(logs) => {
            let log_content = logs
                .iter()
                .map(|log| log.message.as_str())
                .collect::<Vec<&str>>()
                .join("\n");

            match output_path {
                Some(path) => match File::create(path) {
                    Ok(mut file) => match file.write_all(log_content.as_bytes()) {
                        Ok(_) => {
                            println!("Logs successfully written to: {}", path);
                        }
                        Err(e) => {
                            error!("Failed to write logs to file: {}", e);
                            std::process::exit(1);
                        }
                    },
                    Err(e) => {
                        error!("Failed to create file {}: {}", path, e);
                        std::process::exit(1);
                    }
                },
                None => {
                    println!("{}", log_content);
                }
            }
        }
        Err(e) => {
            error!("Failed to get logs for job {}: {}", job_id, e);
            std::process::exit(1);
        }
    }
}
