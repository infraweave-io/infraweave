use anyhow::Result;
use env_common::{
    errors::ModuleError,
    logic::{deprecate_module, precheck_module, publish_module},
};
use env_defs::CloudProvider;
use http_client::{
    http_deprecate_module, http_get_all_latest_modules, http_get_all_versions_for_module,
    http_get_module_version, is_http_mode_enabled, is_not_found_error,
};
use log::{error, info};

use super::{exit_on_err, exit_on_none};
use crate::current_region_handler;

async fn fetch_all_latest_modules(track: &str) -> Result<Vec<env_defs::ModuleResp>> {
    if is_http_mode_enabled() {
        Ok(http_get_all_latest_modules(track).await?)
    } else {
        Ok(current_region_handler()
            .await
            .get_all_latest_module(track)
            .await?)
    }
}

async fn fetch_module_version(
    track: &str,
    module: &str,
    version: &str,
) -> Result<Option<env_defs::ModuleResp>> {
    if is_http_mode_enabled() {
        match http_get_module_version(track, module, version).await {
            Ok(m) => Ok(Some(m)),
            Err(e) if is_not_found_error(&e) => Ok(None),
            Err(e) => Err(e),
        }
    } else {
        Ok(current_region_handler()
            .await
            .get_module_version(module, track, version)
            .await?)
    }
}

async fn fetch_all_module_versions(track: &str, module: &str) -> Result<Vec<env_defs::ModuleResp>> {
    if is_http_mode_enabled() {
        Ok(http_get_all_versions_for_module(track, module).await?)
    } else {
        Ok(current_region_handler()
            .await
            .get_all_module_versions(module, track)
            .await?)
    }
}

async fn do_deprecate_module(
    module: &str,
    track: &str,
    version: &str,
    message: Option<&str>,
) -> Result<()> {
    if is_http_mode_enabled() {
        http_deprecate_module(track, module, version, message.map(|s| s.to_string()))
            .await
            .map(|_| ())
            .map_err(|e| anyhow::anyhow!("Failed to deprecate module: {}", e))
    } else {
        deprecate_module(
            &current_region_handler().await,
            module,
            track,
            version,
            message,
        )
        .await
    }
}

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
                    "Module version {} already exists: {}, but continuing due to --no-fail-on-exist",
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
    exit_on_err(precheck_module(&file.to_string()).await);
    info!("Module prechecked successfully");
}

pub async fn handle_list(track: &str) {
    let modules = exit_on_err(fetch_all_latest_modules(track).await);

    println!(
        "{:<20} {:<20} {:<20} {:<15} {:<15} {:<10}",
        "Module", "ModuleName", "Version", "Track", "Status", "Ref"
    );
    for entry in &modules {
        let status = if entry.deprecated {
            "DEPRECATED"
        } else {
            "Active"
        };
        println!(
            "{:<20} {:<20} {:<20} {:<15} {:<15} {:<10}",
            entry.module, entry.module_name, entry.version, entry.track, status, entry.reference,
        );
    }
}

pub async fn handle_get(module: &str, version: &str) {
    let track = "dev";
    let module = exit_on_none(
        exit_on_err(fetch_module_version(track, module, version).await),
        "Module not found",
    );
    println!(
        "Module: {}",
        serde_json::to_string_pretty(&module).unwrap_or_else(|_| "Failed to serialize".to_string())
    );
    if module.deprecated {
        println!("\n⚠️  WARNING: This module version is DEPRECATED");
        if let Some(msg) = &module.deprecated_message {
            println!("   Reason: {}", msg);
        }
    }
}

pub async fn handle_versions(module: &str, track: &str) {
    let versions = exit_on_err(fetch_all_module_versions(track, module).await);

    if versions.is_empty() {
        println!("No versions found for module {} on track {}", module, track);
        return;
    }

    println!(
        "{:<20} {:<15} {:<30} {}",
        "Version", "Status", "Created", "Message"
    );
    for entry in &versions {
        let status = if entry.deprecated {
            "DEPRECATED"
        } else {
            "Active"
        };
        let message = entry.deprecated_message.as_deref().unwrap_or("");
        println!(
            "{:<20} {:<15} {:<30} {}",
            entry.version, status, entry.timestamp, message
        );
    }
}

pub async fn handle_deprecate(module: &str, track: &str, version: &str, message: Option<&str>) {
    exit_on_err(do_deprecate_module(module, track, version, message).await);
    info!(
        "Module {} version {} in track {} has been deprecated",
        module, version, track
    );
}
