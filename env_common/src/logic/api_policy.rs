use std::path::Path;

use env_defs::{get_policy_identifier, GenericFunctionResponse, PolicyManifest, PolicyResp};
use env_utils::{get_timestamp, merge_json_dicts, semver_parse, validate_policy_schema, zero_pad_semver};

use crate::{interface::CloudHandler, logic::common::handler};

pub async fn publish_policy(manifest_path: &str, environment: &str) -> anyhow::Result<(), anyhow::Error> {
    let policy_yaml_path = Path::new(&manifest_path).join("policy.yaml");
    let manifest =
        std::fs::read_to_string(&policy_yaml_path).expect("Failed to read policy manifest file");

    let policy_yaml =
        serde_yaml::from_str::<PolicyManifest>(&manifest).expect("Failed to parse policy manifest");

    let zip_file = env_utils::get_zip_file(&Path::new(manifest_path), &policy_yaml_path).await?;
    // Encode the zip file content to Base64
    let zip_base64 = base64::encode(&zip_file);

    match validate_policy_schema(&manifest) {
        std::result::Result::Ok(_) => (),
        Err(error) => {
            println!("{}", error);
            std::process::exit(1);
        }
    }

    let policy = PolicyResp {
        environment: environment.to_string(),
        environment_version: format!(
            "{}#{}",
            environment,
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

    if let Ok(latest_policy) = handler().get_newest_policy_version(&policy.policy, &environment).await {
        let manifest_version = semver_parse(&policy.version).unwrap();
        let latest_version = semver_parse(&latest_policy.version).unwrap();

        if manifest_version == latest_version {
            println!(
                "Policy version {} already exists in environment {}",
                manifest_version, environment
            );
            return Err(anyhow::anyhow!(
                "Policy version {} already exists in environment {}",
                manifest_version,
                environment
            ));
        } else if !(manifest_version > latest_version) {
            println!(
                "Policy version {} is older than the latest version {} in environment {}",
                manifest_version, latest_version, environment
            );
            return Err(anyhow::anyhow!(
                "Policy version {} is older than the latest version {} in environment {}",
                manifest_version,
                latest_version,
                environment
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
            &policy.policy, &environment
        );
        println!("Creating new policy version");
    }

    match upload_file_base64(&policy.s3_key, &zip_base64).await {
        Ok(_) => {
            println!("Successfully uploaded policy zip file to S3");
        }
        Err(error) => {
            println!("{}", error);
            std::process::exit(1);
        }
    }

    match insert_policy(&policy).await {
        Ok(_) => {
            println!("Successfully published policy {}", policy.policy);
        }
        Err(error) => {
            println!("{}", error);
            std::process::exit(1);
        }
    }

    println!(
        "Publishing version {} of policy {}",
        policy.version, policy.policy
    );

    Ok(())
}


async fn upload_file_base64(key: &String, base64_content: &String) -> Result<GenericFunctionResponse, anyhow::Error> {
    let payload = serde_json::json!({
        "event": "upload_file_base64",
        "data":
        {
            "key": key,
            "bucket_name": "policies",
            "base64_content": base64_content
        }

    });

    match handler().run_function(&payload).await {
        Ok(response) => Ok(response),
        Err(e) => {
            return Err(anyhow::anyhow!("Failed to read db: {}", e));
        }
    }
}


async fn insert_policy(policy: &PolicyResp) -> anyhow::Result<String> {
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
        "SK": format!("VERSION#{}", zero_pad_semver(&policy.version, 3).unwrap()),
    }))
    .unwrap();

    let policy_value = serde_json::to_value(&policy).unwrap();
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
    }))
    .unwrap();

    // Use the same policy metadata to the current policy version
    merge_json_dicts(&mut current_policy_payload, &policy_value);

    transaction_items.push(serde_json::json!({
        "Put": {
            "TableName": policy_table_placeholder,
            "Item": current_policy_payload
        }
    }));

    // -------------------------
    // Execute the Transaction
    // -------------------------
    let payload = serde_json::json!({
        "event": "transact_write",
        "items": transaction_items,
    });

    println!("Invoking Lambda with payload: {}", payload);

    match handler().run_function(&payload).await {
        Ok(_) => Ok("".to_string()),
        Err(e) => {
            Err(anyhow::anyhow!("Failed to insert policy: {}", e))
        }
    }
}

pub async fn get_all_policies(environment: &str) -> Result<Vec<PolicyResp>, anyhow::Error> {
    handler().get_all_policies(environment).await
}

pub async fn get_policy_download_url(key: &str) -> Result<String, anyhow::Error> {
    handler().get_policy_download_url(key).await
}

pub async fn get_policy(policy: &str, environment: &str, version: &str) -> Result<PolicyResp, anyhow::Error> {
    handler().get_policy(policy, environment, version).await
}
