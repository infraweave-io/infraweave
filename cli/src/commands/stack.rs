use env_common::{
    errors::ModuleError,
    logic::{get_stack_preview, publish_stack},
};
use log::{error, info};

use crate::current_region_handler;

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
