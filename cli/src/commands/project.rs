use log::error;

use crate::current_region_handler;
use env_defs::CloudProvider;

pub async fn handle_get_current() {
    match current_region_handler().await.get_current_project().await {
        Ok(project) => {
            println!(
                "Project: {}",
                serde_json::to_string_pretty(&project).unwrap()
            );
        }
        Err(e) => {
            error!("Failed to insert project: {}", e);
            std::process::exit(1);
        }
    }
}

pub async fn handle_get_all() {
    match current_region_handler().await.get_all_projects().await {
        Ok(projects) => {
            for project in projects {
                println!(
                    "Project: {}",
                    serde_json::to_string_pretty(&project).unwrap()
                );
            }
        }
        Err(e) => {
            error!("Failed to insert project: {}", e);
            std::process::exit(1);
        }
    }
}
