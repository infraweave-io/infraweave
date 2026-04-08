use anyhow::Result;
use env_common::{errors::ModuleError, publish_provider};
use env_defs::CloudProvider;
use http_client::{http_get_all_latest_providers, is_http_mode_enabled};
use log::{error, info};

use super::exit_on_err;
use crate::current_region_handler;

// ── Transport-agnostic fetch helpers ────────────────────────────────────────

async fn fetch_all_latest_providers() -> Result<Vec<env_defs::ProviderResp>> {
    if is_http_mode_enabled() {
        Ok(http_get_all_latest_providers().await?)
    } else {
        Ok(current_region_handler()
            .await
            .get_all_latest_provider()
            .await?)
    }
}

// ── Public handlers ─────────────────────────────────────────────────────────

pub async fn handle_publish(path: &str, version: Option<&str>, no_fail_on_exist: bool) {
    match publish_provider(&current_region_handler().await, path, version).await {
        Ok(_) => {
            info!("Provider published successfully");
        }
        Err(ModuleError::ModuleVersionExists(version, error)) => {
            if no_fail_on_exist {
                info!("Provider version {} already exists: {}, but continuing due to --no-fail-on-exist exits with success", version, error);
            } else {
                error!("Provider already exists, exiting with error: {}", error);
                std::process::exit(1);
            }
        }
        Err(e) => {
            error!("Failed to publish provider: {}", e);
            std::process::exit(1);
        }
    }
}

pub async fn handle_list() {
    let providers = exit_on_err(fetch_all_latest_providers().await);

    println!(
        "{:<20} {:<20} {:<20} {:<15} {:<10}",
        "Provider", "Version", "Config name", "Config alias", "Ref"
    );
    for entry in &providers {
        println!(
            "{:<20} {:<20} {:<20} {:<15} {:<10}",
            entry.name,
            entry.version,
            entry.manifest.spec.provider,
            entry.manifest.spec.alias.clone().unwrap_or("".to_string()),
            entry.reference,
        );
    }
}
