use std::collections::{HashMap, HashSet};

use env_common::{
    errors::ModuleError,
    logic::{build_stack, deprecate_stack, get_stack_preview, publish_stack},
};
use env_utils::to_camel_case;
use log::{error, info};

use crate::current_region_handler;
use env_defs::CloudProvider;

pub async fn handle_preview(path: &str) {
    match get_stack_preview(&current_region_handler().await, &path.to_string()).await {
        Ok(stack_module) => {
            info!("Stack generated successfully");
            println!("{}", stack_module);
        }
        Err(e) => {
            error!("Failed to generate preview for stack: {}", e);
            std::process::exit(1);
        }
    }
}

pub async fn handle_check(path: &str) {
    match build_stack(&current_region_handler().await, path).await {
        Ok((stack_module, _)) => {
            println!(
                "Stack \"{}\" @ {} builds",
                stack_module.manifest.metadata.name, path
            );
            stack_module
                .tf_providers
                .iter()
                .flat_map(|p| p.tf_variables.iter())
                .filter(|v| {
                    !v.nullable
                        && (v.default.is_none()
                            || matches!(v.default, Some(serde_json::Value::Null)))
                })
                .map(|v| &v.name)
                .collect::<HashSet<_>>()
                .iter()
                .for_each(|name| println!("Required variable for provider: {}", name));
            let mut grouped = HashMap::<String, Vec<String>>::new();
            stack_module
                .tf_variables
                .iter()
                .filter(|v| {
                    !v.nullable
                        && (v.default.is_none()
                            || matches!(v.default, Some(serde_json::Value::Null)))
                })
                .map(|v| v.name.clone())
                .for_each(|name| {
                    let (module, field) = name
                        .split_once("__")
                        .expect("Variable should be 'module__field', missing separator");
                    grouped
                        .entry(module.to_string())
                        .or_insert_with(Vec::new)
                        .push(field.to_string());
                });
            for (module, mut fields) in grouped {
                println!("Required variables for {}", module);
                fields.sort();
                fields
                    .iter()
                    .for_each(|f| println!("* {}", to_camel_case(f)));
            }
        }
        Err(e) => {
            error!("Failed to build stack: {}", e);
            std::process::exit(1);
        }
    }
}

pub async fn handle_publish(
    path: &str,
    track: &str,
    version: Option<&str>,
    no_fail_on_exist: bool,
) {
    let handler = &current_region_handler().await;
    match build_stack(handler, path).await {
        Ok((stack_module, stack_zip)) => {
            println!("Built stack");
            match publish_stack(handler, version, track, &stack_module, stack_zip).await {
                Ok(_) => {
                    println!("Published stack")
                }
                Err(ModuleError::ModuleVersionExists(version, error)) => {
                    if no_fail_on_exist {
                        println!(
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
        Err(e) => {
            error!("Failed to build stack: {}", e);
            std::process::exit(1);
        }
    }
}

pub async fn handle_list(track: &str) {
    let stacks = current_region_handler()
        .await
        .get_all_latest_stack(track)
        .await
        .unwrap();
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
    let track = "dev".to_string();
    match current_region_handler()
        .await
        .get_stack_version(stack, &track, version)
        .await
        .unwrap()
    {
        Some(stack) => {
            println!("Stack: {}", serde_json::to_string_pretty(&stack).unwrap());
            if stack.deprecated {
                println!("\n⚠️  WARNING: This stack version is DEPRECATED");
                if let Some(msg) = &stack.deprecated_message {
                    println!("   Reason: {}", msg);
                }
            }
        }
        None => {
            error!("Stack not found");
            std::process::exit(1);
        }
    }
}

pub async fn handle_versions(stack: &str, track: &str) {
    match current_region_handler()
        .await
        .get_all_stack_versions(stack, track)
        .await
    {
        Ok(versions) => {
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
                let message = if let Some(msg) = &entry.deprecated_message {
                    msg.as_str()
                } else {
                    ""
                };
                println!(
                    "{:<20} {:<15} {:<30} {}",
                    entry.version, status, entry.timestamp, message
                );
            }
        }
        Err(e) => {
            error!("Failed to get stack versions: {}", e);
            std::process::exit(1);
        }
    }
}

pub async fn handle_deprecate(stack: &str, track: &str, version: &str, message: Option<&str>) {
    match deprecate_stack(
        &current_region_handler().await,
        stack,
        track,
        version,
        message,
    )
    .await
    {
        Ok(_) => {
            info!(
                "Stack {} version {} in track {} has been deprecated",
                stack, version, track
            );
        }
        Err(e) => {
            error!("Failed to deprecate stack: {}", e);
            std::process::exit(1);
        }
    }
}
