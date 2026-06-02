use base64::engine::general_purpose::STANDARD as base64;
use base64::Engine;
use std::path::Path;

use env_defs::{
    get_policy_identifier, CloudProvider, GenericFunctionResponse, PolicyManifest, PolicyResp,
};
use env_utils::{
    get_timestamp, merge_json_dicts, semver_parse, validate_policy_schema, zero_pad_semver,
};

use crate::interface::GenericCloudHandler;

const DEFAULT_POLICY_ENVIRONMENT: &str = "default";

pub async fn publish_policy(
    handler: &GenericCloudHandler,
    manifest_path: &str,
    _environment: &str,
) -> anyhow::Result<(), anyhow::Error> {
    let policy_yaml_path = Path::new(&manifest_path).join("policy.yaml");
    let manifest =
        std::fs::read_to_string(&policy_yaml_path).expect("Failed to read policy manifest file");

    let policy_yaml =
        serde_yaml::from_str::<PolicyManifest>(&manifest).expect("Failed to parse policy manifest");

    let zip_file = env_utils::get_zip_file(Path::new(manifest_path), &policy_yaml_path).await?;
    // Encode the zip file content to Base64
    let zip_base64 = base64.encode(&zip_file);

    match validate_policy_schema(&manifest) {
        std::result::Result::Ok(_) => (),
        Err(error) => {
            return Err(anyhow::anyhow!("{}", error));
        }
    }

    let policy = PolicyResp {
        environment: DEFAULT_POLICY_ENVIRONMENT.to_string(),
        environment_version: format!(
            "{}#{}",
            DEFAULT_POLICY_ENVIRONMENT,
            zero_pad_semver(policy_yaml.spec.version.as_str(), 3).unwrap()
        ),
        version: policy_yaml.spec.version.clone(),
        timestamp: get_timestamp(),
        policy: policy_yaml.metadata.name.clone(),
        policy_name: policy_yaml.spec.policy_name.clone(),
        description: policy_yaml.spec.description.clone(),
        reference: policy_yaml.spec.reference.clone(),
        manifest: policy_yaml.clone(),
        data: policy_yaml.spec.data.clone(),
        s3_key: format!(
            "{}/{}-{}.zip",
            &policy_yaml.metadata.name, &policy_yaml.metadata.name, &policy_yaml.spec.version
        ), // s3_key -> "{policy}/{policy}-{version}.zip"
    };

    if http_client::is_http_mode_enabled() {
        let policy_json = serde_json::to_value(&policy)?;
        http_client::http_publish_policy(&zip_base64, &policy_json)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to upload policy: {}", e))?;
        return Ok(());
    }

    let all_regions = handler.get_all_regions().await?;
    server_publish_policy(handler, &policy, &zip_base64, &all_regions).await
}

/// Server-side publish: validates version ordering, then uploads to pre-resolved regions.
/// Called by the internal-api after region discovery has happened outside publish scope.
pub async fn server_publish_policy(
    handler: &GenericCloudHandler,
    policy: &PolicyResp,
    zip_base64: &str,
    all_regions: &[String],
) -> anyhow::Result<(), anyhow::Error> {
    if all_regions.is_empty() {
        return Err(anyhow::anyhow!(
            "At least one region is required to publish policy"
        ));
    }

    if let Ok(latest_policy) = handler
        .get_newest_policy_version(&policy.policy, &policy.environment)
        .await
    {
        let manifest_version = semver_parse(&policy.version)?;
        let latest_version = semver_parse(&latest_policy.version)?;

        if manifest_version == latest_version {
            println!(
                "Policy version {} already exists in environment {}",
                manifest_version, policy.environment
            );
            return Err(anyhow::anyhow!(
                "Policy version {} already exists in environment {}",
                manifest_version,
                policy.environment
            ));
        } else if manifest_version <= latest_version {
            println!(
                "Policy version {} is older than the latest version {} in environment {}",
                manifest_version, latest_version, policy.environment
            );
            return Err(anyhow::anyhow!(
                "Policy version {} is older than the latest version {} in environment {}",
                manifest_version,
                latest_version,
                policy.environment
            ));
        } else {
            println!(
                "Policy version {} is confirmed to be the newest version",
                manifest_version
            );
        }
    } else {
        println!(
            "No policy found with policy: {} and environment: {}",
            policy.policy, policy.environment
        );
        println!("Creating new policy version");
    }

    println!("Publishing policy to all regions...");

    for region in all_regions.iter() {
        let region_handler = handler.copy_with_region(region).await;

        match upload_file_base64(&region_handler, &policy.s3_key, zip_base64).await {
            Ok(_) => {
                println!(
                    "Successfully uploaded policy zip file to S3 in region {}",
                    region
                );
            }
            Err(error) => {
                println!("Failed to upload policy to region {}: {}", region, error);
                return Err(error);
            }
        }

        match insert_policy(&region_handler, &policy).await {
            Ok(_) => {
                println!(
                    "Successfully published policy {} in region {}",
                    policy.policy, region
                );
            }
            Err(error) => {
                println!("Failed to insert policy in region {}: {}", region, error);
                return Err(error);
            }
        }
    }

    println!(
        "Publishing version {} of policy {} completed in all regions",
        policy.version, policy.policy
    );

    Ok(())
}

async fn upload_file_base64<T: CloudProvider>(
    handler: &T,
    key: &str,
    base64_content: &str,
) -> Result<GenericFunctionResponse, anyhow::Error> {
    let payload = env_defs::upload_file_base64_event(key, "policies", base64_content);

    match handler.run_function(&payload).await {
        Ok(response) => Ok(response),
        Err(e) => Err(anyhow::anyhow!("Failed to read db: {}", e)),
    }
}

async fn insert_policy<T: CloudProvider>(
    handler: &T,
    policy: &PolicyResp,
) -> anyhow::Result<String> {
    let policy_table_placeholder = "policies";

    let mut transaction_items = vec![];

    let id: String = format!(
        "POLICY#{}",
        get_policy_identifier(&policy.policy, &policy.environment)
    );

    // -------------------------
    // Policy metadata
    // -------------------------
    let mut policy_payload = serde_json::to_value(serde_json::json!({
        "PK": id.clone(),
        "SK": format!("VERSION#{}", zero_pad_semver(&policy.version, 3)?),
    }))?;

    let policy_value = serde_json::to_value(policy)?;
    merge_json_dicts(&mut policy_payload, &policy_value);

    transaction_items.push(serde_json::json!({
        "Put": {
            "TableName": policy_table_placeholder,
            "Item": policy_payload
        }
    }));

    // -------------------------
    // Current policy version
    // -------------------------
    let mut current_policy_payload = serde_json::to_value(serde_json::json!({
        "PK": "CURRENT",
        "SK": id.clone(),
    }))?;

    // Use the same policy metadata to the current policy version
    merge_json_dicts(&mut current_policy_payload, &policy_value);

    transaction_items.push(serde_json::json!({
        "Put": {
            "TableName": policy_table_placeholder,
            "Item": current_policy_payload
        }
    }));

    let items = serde_json::to_value(&transaction_items)?;
    let payload = env_defs::transact_write_event(&items);

    match handler.run_function(&payload).await {
        Ok(_) => Ok("".to_string()),
        Err(e) => Err(anyhow::anyhow!("Failed to insert policy: {}", e)),
    }
}
