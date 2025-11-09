use env_common::{
    errors::ModuleError,
    logic::{deprecate_module, precheck_module, publish_module},
};
use log::{error, info};

use crate::current_region_handler;
use env_defs::CloudProvider;

pub async fn handle_publish(
    path: &str,
    track: &str,
    version: Option<&str>,
    no_fail_on_exist: bool,
) {
    match publish_module(&current_region_handler().await, path, track, version, None).await {
        Ok(_) => {
            info!("Module published successfully");
        }
        Err(ModuleError::ModuleVersionExists(version, error)) => {
            if no_fail_on_exist {
                info!(
                    "Module version {} already exists: {}, but continuing due to --no-fail-on-exist exits with success",
                    version, error
                );
            } else {
                error!("Module already exists, exiting with error: {}", error);
                std::process::exit(1);
            }
        }
        Err(e) => {
            error!("Failed to publish module: {}", e);
            std::process::exit(1);
        }
    }
}

pub async fn handle_precheck(file: &str) {
    match precheck_module(&file.to_string()).await {
        Ok(_) => {
            info!("Module prechecked successfully");
        }
        Err(e) => {
            error!("Failed during module precheck: {}", e);
            std::process::exit(1);
        }
    }
}

pub async fn handle_list(track: &str) {
    let modules = current_region_handler()
        .await
        .get_all_latest_module(track)
        .await
        .unwrap();
    println!(
        "{:<20} {:<20} {:<20} {:<15} {:<10}",
        "Module", "ModuleName", "Version", "Track", "Ref"
    );
    for entry in &modules {
        println!(
            "{:<20} {:<20} {:<20} {:<15} {:<10}",
            entry.module, entry.module_name, entry.version, entry.track, entry.reference,
        );
    }
}

pub async fn handle_get(module: &str, version: &str) {
    let track = "dev".to_string();
    match current_region_handler()
        .await
        .get_module_version(module, &track, version)
        .await
        .unwrap()
    {
        Some(module) => {
            println!("Module: {}", serde_json::to_string_pretty(&module).unwrap());
        }
        None => {
            error!("Module not found");
            std::process::exit(1);
        }
    }
}

pub async fn handle_deprecate(module: &str, track: &str, version: &str, message: Option<&str>) {
    match deprecate_module(
        &current_region_handler().await,
        module,
        track,
        version,
        message,
    )
    .await
    {
        Ok(_) => {
            info!(
                "Module {} version {} in track {} has been deprecated",
                module, version, track
            );
        }
        Err(e) => {
            error!("Failed to deprecate module: {}", e);
            std::process::exit(1);
        }
    }
}
