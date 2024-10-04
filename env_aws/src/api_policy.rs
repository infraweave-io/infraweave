use anyhow::Result;
use chrono::Local;
use env_defs::{PolicyManifest, PolicyResp};
use env_utils::{merge_json_dicts, semver_parse, validate_policy_schema, zero_pad_semver};
use log::error;
use serde_json::Value;
use std::path::Path;

use crate::api::run_lambda;

use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
struct ApiPublishPayloadLambdaPayload {
    event: String,
    manifest: String,
    environment: String,
    description: String,
    reference: String,
    zip_file_base64: String,
    data: Value,
}

#[derive(Debug, Serialize, Deserialize)]
struct ApiGetPolicyLambdaPayload {
    deployment_id: String,
    query: Value,
}

pub async fn publish_policy(
    manifest_path: &String,
    environment: &String,
) -> anyhow::Result<(), anyhow::Error> {
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
        environment: environment.clone(),
        environment_version: format!(
            "{}#{}",
            environment.clone(),
            zero_pad_semver(policy_yaml.spec.version.as_str(), 3).unwrap()
        ),
        version: policy_yaml.spec.version.clone(),
        timestamp: Local::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
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

    if let Ok(latest_policy) = get_newest_policy_version(&policy.policy, &environment).await {
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

pub async fn list_policy(environment: &str) -> Result<Vec<PolicyResp>, anyhow::Error> {
    let pk: String = "CURRENT".to_string();
    let response = read_db(serde_json::json!({
        "KeyConditionExpression": "PK = :current",
        "ExpressionAttributeValues": {":current": pk},
        // "Limit": 1,
    }))
    .await?;

    let items = response.get("Items").expect("Items not found");

    if let Some(policies) = items.as_array() {
        let policies_string =
            serde_json::to_string(policies).expect("Failed to convert policies to string");
        let policies: Vec<PolicyResp> =
            serde_json::from_str(&policies_string).expect("Failed to parse inner JSON string");

        println!(
            "{:<20} {:<20} {:<20} {:<15} {:<10} {:<30}",
            "Policy", "PolicyName", "Version", "Environment", "Ref", "Description"
        );
        for entry in &policies {
            println!(
                "{:<20} {:<20} {:<20} {:<15} {:<10} {:<30}",
                entry.policy,
                entry.policy_name,
                entry.version,
                entry.environment,
                entry.reference,
                entry.description
            );
        }
        return Ok(policies);
    } else {
        println!("No payload in response");
    }
    Ok([].to_vec())
}

pub async fn get_policy_download_url(key: &String) -> Result<String, anyhow::Error> {
    let payload = serde_json::json!({
        "event": "generate_presigned_url",
        "data":{
            "key": key,
            "bucket_name": "policies",
            "expires_in": 60,
        }
    });

    let response = match run_lambda(payload).await {
        Ok(response) => response,
        Err(e) => {
            error!("Failed to read db: {}", e);
            println!("Failed to read db: {}", e);
            return Err(anyhow::anyhow!("Failed to read db: {}", e));
        }
    };

    let url = response
        .get("url")
        .expect("Presigned url not found")
        .as_str()
        .unwrap()
        .to_string();

    Ok(url)
}

pub async fn get_policy_version(policy: &String, environment: &String, version: &String) -> anyhow::Result<PolicyResp> {
    let id: String = format!(
        "POLICY#{}",
        get_identifier(&policy, &environment)
    );
    let version_id = format!("VERSION#{}", zero_pad_semver(&version, 3).unwrap());
    let response = read_db(serde_json::json!({
        "KeyConditionExpression": "PK = :policy AND SK = :sk",
        "ExpressionAttributeValues": {":policy": id, ":sk": version_id},
        "Limit": 1,
    }))
    .await?;

    let items = response.get("Items").expect("Items not found");

    let policies_string =
        serde_json::to_string(items).expect("Failed to convert policies to string");
    let policies: Vec<PolicyResp> =
        serde_json::from_str(&policies_string).expect("Failed to parse inner JSON string");

    if policies.len() == 0 {
        println!(
            "No policy found with policy: {} and version: {}",
            policy, version
        );
        return Err(anyhow::anyhow!(
            "No policy found with policy: {} and version: {}",
            policy,
            version
        ));
    }

    let policy = &policies[0];

    let yaml_string = serde_yaml::to_string(&policy.manifest).unwrap();
    println!("Information\n------------");
    println!("Policy: {}", policy.policy);
    println!("Version: {}", policy.version);
    println!("Description: {}", policy.description);
    println!("Reference: {}", policy.reference);
    println!("\n");

    println!("{}", yaml_string);
    return Ok(policy.clone());
}

pub async fn get_current_policy_version(
    policy: &String,
    environment: &String,
) -> anyhow::Result<PolicyResp> {
    let pk: String = "CURRENT".to_string();
    let response = read_db(serde_json::json!({
        "KeyConditionExpression": "PK = :current AND SK = :policy",
        "ExpressionAttributeValues": {":current": pk, ":policy": policy},
        "Limit": 1,
    }))
    .await?;

    let items = response.get("Items").expect("Items not found");

    let policies_string =
        serde_json::to_string(items).expect("Failed to convert policies to string");
    let policies: Vec<PolicyResp> =
        serde_json::from_str(&policies_string).expect("Failed to parse inner JSON string");

    if policies.len() == 0 {
        println!(
            "No policy found with policy: {} and environment: {}",
            policy, environment
        );
        return Err(anyhow::anyhow!(
            "No policy found with policy: {} and environment: {}",
            policy,
            environment
        ));
    }

    let policy = &policies[0];

    let yaml_string = serde_yaml::to_string(&policy.manifest).unwrap();
    println!("Information\n------------");
    println!("Policy: {}", policy.policy);
    println!("Version: {}", policy.version);
    println!("Description: {}", policy.description);
    println!("Reference: {}", policy.reference);
    println!("\n");

    println!("{}", yaml_string);
    return Ok(policy.clone());
}

pub async fn get_newest_policy_version(
    policy: &String,
    environment: &String,
) -> anyhow::Result<PolicyResp> {
    let id: String = format!(
        "POLICY#{}",
        get_identifier(&policy, &environment)
    );
    let response = read_db(serde_json::json!({
        "KeyConditionExpression": "PK = :policy",
        "ExpressionAttributeValues": {":policy": id},
        "ScanIndexForward": false,
        "Limit": 1,
    }))
    .await?;

    let items = response.get("Items").expect("Items not found");

    let policies_string =
        serde_json::to_string(items).expect("Failed to convert policies to string");
    let policies: Vec<PolicyResp> =
        serde_json::from_str(&policies_string).expect("Failed to parse inner JSON string");

    if policies.len() == 0 {
        println!(
            "No policy found with policy: {} and environment: {}",
            policy, environment
        );
        return Err(anyhow::anyhow!(
            "No policy found with policy: {} and environment: {}",
            policy,
            environment
        ));
    }

    let policy = &policies[0];

    let yaml_string = serde_yaml::to_string(&policy.manifest).unwrap();
    println!("Information\n------------");
    println!("Policy: {}", policy.policy);
    println!("Version: {}", policy.version);
    println!("Description: {}", policy.description);
    println!("Reference: {}", policy.reference);
    println!("\n");

    println!("{}", yaml_string);
    return Ok(policy.clone());
}

async fn read_db(query: Value) -> Result<Value, anyhow::Error> {
    let payload = ApiGetPolicyLambdaPayload {
        deployment_id: "".to_string(),
        query: query,
    };

    let payload = serde_json::json!({
        "event": "read_db",
        "table": "policies",
        "data": payload
    });

    let response = match run_lambda(payload).await {
        Ok(response) => response,
        Err(e) => {
            error!("Failed to read db: {}", e);
            println!("Failed to read db: {}", e);
            return Err(anyhow::anyhow!("Failed to read db: {}", e));
        }
    };

    Ok(response)
}

async fn upload_file_base64(key: &String, base64_content: &String) -> Result<Value, anyhow::Error> {
    let payload = serde_json::json!({
        "event": "upload_file_base64",
        "data":
        {
            "key": key,
            "bucket_name": "policies",
            "base64_content": base64_content
        }

    });

    let response = match run_lambda(payload).await {
        Ok(response) => response,
        Err(e) => {
            error!("Failed to read db: {}", e);
            println!("Failed to read db: {}", e);
            return Err(anyhow::anyhow!("Failed to read db: {}", e));
        }
    };

    Ok(response)
}

pub async fn insert_policy(policy: &PolicyResp) -> anyhow::Result<String> {
    let POLICY_TABLE_NAME = "Policies-eu-central-1-dev";

    let mut transaction_items = vec![];

    let id: String = format!(
        "POLICY#{}",
        get_identifier(&policy.policy, &policy.environment)
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
            "TableName": POLICY_TABLE_NAME,
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
            "TableName": POLICY_TABLE_NAME,
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

    match run_lambda(payload).await {
        Ok(_) => Ok("".to_string()),
        Err(e) => {
            error!("Failed to insert policy: {}", e);
            Err(anyhow::anyhow!("Failed to insert policy: {}", e))
        }
    }
}

fn get_identifier(deployment_id: &str, environment: &str) -> String {
    format!("{}::{}", environment, deployment_id)
}
