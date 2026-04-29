use anyhow::{anyhow, Result};
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
];

pub const DEFAULT_BUCKET_NAMES: &[(&str, &str)] = &[
    ("MODULE_S3_BUCKET", "modules"),
    ("POLICY_S3_BUCKET", "policies"),
    ("CHANGE_RECORD_S3_BUCKET", "change-records"),
    ("PROVIDERS_S3_BUCKET", "providers"),
];

#[allow(dead_code)]
pub async fn get_region() -> String {
    if let Ok(region_env) = std::env::var("AWS_REGION") {
        return region_env;
    }

    {
        panic!("AWS_REGION environment variable must be set");
    }
}

/// Get DynamoDB table name from environment variable
pub fn get_table_name(table_type: &str) -> Result<String> {
    let env_var = match table_type.to_lowercase().as_str() {
        "events" => "DYNAMODB_EVENTS_TABLE_NAME",
        "modules" => "DYNAMODB_MODULES_TABLE_NAME",
        "deployments" => "DYNAMODB_DEPLOYMENTS_TABLE_NAME",
        "policies" => "DYNAMODB_POLICIES_TABLE_NAME",
        "change_records" | "changerecords" => "DYNAMODB_CHANGE_RECORDS_TABLE_NAME",
        "config" => "DYNAMODB_CONFIG_TABLE_NAME",
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

/// Get table name with region adjustment (replaces current region with target region in the name)
pub fn get_table_name_for_region(table_type: &str, region: Option<&str>) -> Result<String> {
    let table_name = get_table_name(table_type)?;

    if let Some(target_region) = region {
        let current_region =
            std::env::var("AWS_REGION").unwrap_or_else(|_| "us-west-2".to_string());

        if target_region != current_region && table_name.contains(&current_region) {
            let new_table_name = table_name.replace(&current_region, target_region);
            info!(
                "Switched table name from '{}' to '{}' for region '{}'",
                table_name, new_table_name, target_region
            );
            return Ok(new_table_name);
        }
    }

    Ok(table_name)
}

/// Get S3 bucket name from environment variable, falling back to defaults for local dev
pub fn get_bucket_name(bucket_type: &str) -> Result<String> {
    let env_var = match bucket_type.to_lowercase().as_str() {
        "modules" => "MODULE_S3_BUCKET",
        "policies" => "POLICY_S3_BUCKET",
        "change_records" | "changerecords" => "CHANGE_RECORD_S3_BUCKET",
        "providers" => "PROVIDERS_S3_BUCKET",
        _ => return Err(anyhow!("Unknown bucket type: {}", bucket_type)),
    };

    match std::env::var(env_var) {
        Ok(bucket_name) => Ok(bucket_name),
        Err(_) => {
            let default_name = DEFAULT_BUCKET_NAMES
                .iter()
                .find(|(env, _)| *env == env_var)
                .map(|(_, name)| *name)
                .ok_or_else(|| anyhow!("Unknown bucket type: {}", bucket_type))?;

            info!(
                "Using default bucket name for local development: {}",
                default_name
            );
            Ok(default_name.to_string())
        }
    }
}

/// Get S3 bucket name with region adjustment
pub fn get_bucket_name_for_region(bucket_type: &str, region: &str) -> Result<String> {
    let bucket_name = get_bucket_name(bucket_type)?;
    let current_region = std::env::var("AWS_REGION").unwrap_or_else(|_| "us-west-2".to_string());
    let updated = bucket_name.replace(&format!("-{}-", current_region), &format!("-{}-", region));
    info!(
        "Bucket name for region '{}': {} -> {}",
        region, bucket_name, updated
    );
    Ok(updated)
}
