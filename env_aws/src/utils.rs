#[cfg(not(feature = "direct"))]
use aws_config::meta::region::RegionProviderChain;

#[cfg(feature = "direct")]
use anyhow::{anyhow, Result};
#[cfg(feature = "direct")]
use log::info;

/// Default table and bucket names for local development
/// Single source of truth used by both internal-api local setup and CLI direct access
pub const DEFAULT_TABLE_NAMES: &[(&str, &str)] = &[
    ("DYNAMODB_EVENTS_TABLE_NAME", "events"),
    ("DYNAMODB_MODULES_TABLE_NAME", "modules"),
    ("DYNAMODB_POLICIES_TABLE_NAME", "policies"),
    ("DYNAMODB_DEPLOYMENTS_TABLE_NAME", "deployments"),
    ("DYNAMODB_CHANGE_RECORDS_TABLE_NAME", "change-records"),
    ("DYNAMODB_CONFIG_TABLE_NAME", "config"),
    ("DYNAMODB_JOBS_TABLE_NAME", "jobs"),
    ("DYNAMODB_PERMISSIONS_TABLE_NAME", "permissions"),
];

pub const DEFAULT_BUCKET_NAMES: &[(&str, &str)] = &[
    ("MODULE_S3_BUCKET", "modules"),
    ("POLICY_S3_BUCKET", "policies"),
    ("CHANGE_RECORD_S3_BUCKET", "change-records"),
    ("PROVIDERS_S3_BUCKET", "providers"),
];

#[allow(dead_code)]
pub async fn get_region() -> String {
    // Check if HTTP mode is enabled via config file
    if crate::http_client::is_http_mode_enabled() {
        // In HTTP API mode, return dummy region - we don't use AWS SDK
        return "us-east-1".to_string();
    }

    if let Ok(region_env) = std::env::var("AWS_REGION") {
        return region_env;
    }

    #[cfg(feature = "direct")]
    {
        panic!("AWS_REGION environment variable must be set");
    }

    #[cfg(not(feature = "direct"))]
    {
        let region_provider = RegionProviderChain::default_provider().or_default_provider();
        let region = match region_provider.region().await {
            Some(region) => region,
            None => {
                eprintln!("No region found, did you forget to set AWS_REGION?");
                std::process::exit(1);
            }
        };
        region.to_string()
    }
}

/// Get S3 bucket name from environment variable and adjust for target region
#[cfg(feature = "direct")]
pub fn get_bucket_name_from_env(bucket_type: &str, target_region: &str) -> Option<String> {
    let env_var = match bucket_type {
        "modules" => "MODULE_S3_BUCKET",
        "policies" => "POLICY_S3_BUCKET",
        "providers" => "PROVIDERS_S3_BUCKET",
        _ => return None,
    };

    // Try environment variable, fall back to default bucket name for local mode
    let base_bucket = std::env::var(env_var).unwrap_or_else(|_| {
        let default_name = DEFAULT_BUCKET_NAMES
            .iter()
            .find(|(env, _)| *env == env_var)
            .map(|(_, name)| name.to_string())
            .unwrap_or_else(|| bucket_type.to_string());
        info!(
            "Using default bucket name for local development: {}",
            default_name
        );
        default_name
    });

    let current_region = std::env::var("AWS_REGION").ok();

    let result = match current_region {
        Some(curr_region) if curr_region != target_region => {
            base_bucket.replace(&curr_region, target_region)
        }
        _ => base_bucket,
    };

    info!(
        "Resolved bucket for '{}' in region '{}': {}",
        bucket_type, target_region, result
    );
    Some(result)
}

/// Get DynamoDB table name from environment variable
#[cfg(feature = "direct")]
pub fn get_table_name(table_type: &str) -> Result<String> {
    let env_var = match table_type.to_lowercase().as_str() {
        "events" => "DYNAMODB_EVENTS_TABLE_NAME",
        "modules" => "DYNAMODB_MODULES_TABLE_NAME",
        "deployments" => "DYNAMODB_DEPLOYMENTS_TABLE_NAME",
        "policies" => "DYNAMODB_POLICIES_TABLE_NAME",
        "change_records" | "changerecords" => "DYNAMODB_CHANGE_RECORDS_TABLE_NAME",
        "config" => "DYNAMODB_CONFIG_TABLE_NAME",
        "jobs" => "DYNAMODB_JOBS_TABLE_NAME",
        _ => return Err(anyhow!("Unknown table type: {}", table_type)),
    };

    // Try to get from environment variable, fall back to default table names for local development
    match std::env::var(env_var) {
        Ok(table_name) => Ok(table_name),
        Err(_) => {
            // Use default table name from centralized configuration
            let default_name = DEFAULT_TABLE_NAMES
                .iter()
                .find(|(env, _)| *env == env_var)
                .map(|(_, name)| *name)
                .ok_or_else(|| anyhow!("Unknown table type: {}", table_type))?;

            info!(
                "Using default table name for local development: {}",
                default_name
            );
            Ok(default_name.to_string())
        }
    }
}

// #[derive(PartialEq)]
// pub enum ModuleType {
//     Module,
//     Stack,
// }
