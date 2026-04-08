use anyhow::Result;
use env_common::{
    errors::ModuleError,
    logic::{deprecate_stack, get_stack_preview, publish_stack},
};
use env_defs::CloudProvider;
use http_client::{
    http_deprecate_stack, http_get_all_latest_stacks, http_get_all_versions_for_stack,
    http_get_stack_version, is_http_mode_enabled, is_not_found_error,
};
use log::{error, info};

use super::{exit_on_err, exit_on_none};
use crate::current_region_handler;

// ── Transport-agnostic fetch helpers ────────────────────────────────────────

async fn fetch_all_latest_stacks(track: &str) -> Result<Vec<env_defs::ModuleResp>> {
    if is_http_mode_enabled() {
        Ok(http_get_all_latest_stacks(track).await?)
    } else {
        Ok(current_region_handler()
            .await
            .get_all_latest_stack(track)
            .await?)
    }
}

async fn fetch_stack_version(
    track: &str,
    stack: &str,
    version: &str,
) -> Result<Option<env_defs::ModuleResp>> {
    if is_http_mode_enabled() {
        match http_get_stack_version(track, stack, version).await {
            Ok(s) => Ok(Some(s)),
            Err(e) if is_not_found_error(&e) => Ok(None),
            Err(e) => Err(e),
        }
    } else {
        Ok(current_region_handler()
            .await
            .get_stack_version(stack, track, version)
            .await?)
    }
}

async fn fetch_all_stack_versions(track: &str, stack: &str) -> Result<Vec<env_defs::ModuleResp>> {
    if is_http_mode_enabled() {
        Ok(http_get_all_versions_for_stack(track, stack).await?)
    } else {
        Ok(current_region_handler()
            .await
            .get_all_stack_versions(stack, track)
            .await?)
    }
}

async fn do_deprecate_stack(
    stack: &str,
    track: &str,
    version: &str,
    message: Option<&str>,
) -> Result<()> {
    if is_http_mode_enabled() {
        http_deprecate_stack(track, stack, version, message.map(|s| s.to_string()))
            .await
            .map(|_| ())
            .map_err(|e| anyhow::anyhow!("Failed to deprecate stack: {}", e))
    } else {
        deprecate_stack(
            &current_region_handler().await,
            stack,
            track,
            version,
            message,
        )
        .await
    }
}

// ── Public handlers ─────────────────────────────────────────────────────────

pub async fn handle_preview(path: &str) {
    let stack_module =
        exit_on_err(get_stack_preview(&current_region_handler().await, &path.to_string()).await);
    info!("Stack generated successfully");
    println!("{}", stack_module);
}

pub async fn handle_publish(
    path: &str,
    track: &str,
    version: Option<&str>,
    no_fail_on_exist: bool,
) {
    match publish_stack(&current_region_handler().await, path, track, version, None).await {
        Ok(_) => {
            info!("Stack published successfully");
        }
        Err(ModuleError::ModuleVersionExists(version, error)) => {
            if no_fail_on_exist {
                info!(
                    "Stack version {} already exists: {}, but continuing due to --no-fail-on-exist exits with success",
                    version, error
                );
            } else {
                error!("Stack already exists, exiting with error: {}", error);
                std::process::exit(1);
            }
        }
        Err(e) => {
            error!("Failed to publish stack: {}", e);
            std::process::exit(1);
        }
    }
}

pub async fn handle_list(track: &str) {
    let stacks = exit_on_err(fetch_all_latest_stacks(track).await);

    println!(
        "{:<20} {:<20} {:<20} {:<15} {:<15} {:<10}",
        "Stack", "StackName", "Version", "Track", "Status", "Ref"
    );
    for entry in &stacks {
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

pub async fn handle_get(stack: &str, version: &str) {
    let track = "dev";
    let stack = exit_on_none(
        exit_on_err(fetch_stack_version(track, stack, version).await),
        "Stack not found",
    );
    println!("Stack: {}", serde_json::to_string_pretty(&stack).unwrap());
    if stack.deprecated {
        println!("\n⚠️  WARNING: This stack version is DEPRECATED");
        if let Some(msg) = &stack.deprecated_message {
            println!("   Reason: {}", msg);
        }
    }
}

pub async fn handle_versions(stack: &str, track: &str) {
    let versions = exit_on_err(fetch_all_stack_versions(track, stack).await);

    if versions.is_empty() {
        println!("No versions found for stack {} on track {}", stack, track);
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

pub async fn handle_deprecate(stack: &str, track: &str, version: &str, message: Option<&str>) {
    exit_on_err(do_deprecate_stack(stack, track, version, message).await);
    info!(
        "Stack {} version {} in track {} has been deprecated",
        stack, version, track
    );
}
