use env_common::logic::publish_policy;
use log::{error, info};

use crate::current_region_handler;
use env_defs::CloudProvider;

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
    let policies = current_region_handler()
        .await
        .get_all_policies(environment)
        .await
        .unwrap();
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
    match current_region_handler()
        .await
        .get_policy(policy, environment, version)
        .await
    {
        Ok(policy) => {
            println!("Policy: {}", serde_json::to_string_pretty(&policy).unwrap());
        }
        Err(e) => {
            error!("Failed to get policy: {}", e);
            std::process::exit(1);
        }
    }
}
