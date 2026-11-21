use env_common::interface::GenericCloudHandler;
use env_common::logic::validate_and_prepare_claim;
use env_defs::ExtraData;
use kube::api::DynamicObject;
use log::{error, info};
use std::env;

/// Validates a claim before it's admitted to Kubernetes
/// Returns (is_valid, message)
pub async fn validate_claim(
    handler: &GenericCloudHandler,
    claim: &DynamicObject,
) -> (bool, String) {
    let claim_name = claim.metadata.name.as_deref().unwrap_or("");
    let namespace = claim.metadata.namespace.as_deref().unwrap_or("default");

    info!(
        "Validating claim: {} in namespace: {}",
        claim_name, namespace
    );

    // Convert DynamicObject to serde_yaml::Value
    let yaml_value = match serde_json::to_value(claim) {
        Ok(json_val) => match serde_yaml::to_value(&json_val) {
            Ok(yaml_val) => yaml_val,
            Err(e) => {
                let msg = format!("Failed to convert claim to YAML: {}", e);
                error!("{}", msg);
                return (false, msg);
            }
        },
        Err(e) => {
            let msg = format!("Failed to convert claim to JSON: {}", e);
            error!("{}", msg);
            return (false, msg);
        }
    };

    // Use cluster ID as environment to match operator behavior
    // Format: k8s-{cluster_id}/{namespace}
    let cluster_id = env::var("INFRAWEAVE_CLUSTER_ID").unwrap_or_else(|_| "cluster-id".to_string());
    let environment = format!("k8s-{}/{}", cluster_id, namespace);

    info!("Using environment: {}", environment);

    let validation_result = validate_and_prepare_claim(
        handler,
        &yaml_value,
        &environment,
        "apply",
        vec![],
        ExtraData::None,
        "main",
    )
    .await;

    match validation_result {
        Ok((_deployment_id, _payload)) => {
            info!("Claim '{}' validated successfully", claim_name);
            (true, "Claim validated successfully".to_string())
        }
        Err(e) => {
            let msg = format!("Validation failed: {}", e);
            error!("{}", msg);
            (false, msg)
        }
    }
}
