use anyhow::Result;
use env_common::logic::publish_policy;
use env_defs::CloudProvider;
use http_client::{http_get_policies, http_get_policy_version, is_http_mode_enabled};
use log::{error, info};

use super::exit_on_err;
use crate::current_region_handler;

async fn fetch_all_policies(environment: &str) -> Result<Vec<env_defs::PolicyResp>> {
    if is_http_mode_enabled() {
        http_get_policies(environment)
            .await?
            .into_iter()
            .map(|v| serde_json::from_value(v).map_err(Into::into))
            .collect()
    } else {
        Ok(current_region_handler()
            .await
            .get_all_policies(environment)
            .await?)
    }
}

async fn fetch_policy(
    policy: &str,
    environment: &str,
    version: &str,
) -> Result<env_defs::PolicyResp> {
    if is_http_mode_enabled() {
        let value = http_get_policy_version(environment, policy, version).await?;
        Ok(serde_json::from_value(value)?)
    } else {
        Ok(current_region_handler()
            .await
            .get_policy(policy, environment, version)
            .await?)
    }
}

pub async fn handle_publish(file: &str, environment: &str) {
    match publish_policy(&current_region_handler().await, file, environment).await {
        Ok(_) => {
            info!("Policy published successfully");
        }
        Err(e) => {
            error!("Failed to publish policy: {}", e);
            std::process::exit(1);
        }
    }
}

pub async fn handle_list(environment: &str) {
    let policies = exit_on_err(fetch_all_policies(environment).await);

    println!(
        "{:<30} {:<20} {:<20} {:<15} {:<10}",
        "Policy", "PolicyName", "Version", "Environment", "Ref"
    );
    for entry in &policies {
        println!(
            "{:<30} {:<20} {:<20} {:<15} {:<10}",
            entry.policy, entry.policy_name, entry.version, entry.environment, entry.reference,
        );
    }
}

pub async fn handle_get(policy: &str, environment: &str, version: &str) {
    let policy = exit_on_err(fetch_policy(policy, environment, version).await);
    println!("Policy: {}", serde_json::to_string_pretty(&policy).unwrap());
}
