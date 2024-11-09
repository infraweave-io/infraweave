use aws_sdk_lambda::primitives::Blob;
use aws_sdk_lambda::types::InvocationType;
use aws_sdk_lambda::Client;
use env_defs::{get_change_record_identifier, get_deployment_identifier, get_event_identifier, get_module_identifier, get_policy_identifier, GenericFunctionResponse};
use env_utils::{get_epoch, sanitize_payload_for_logging, zero_pad_semver};
use log::{error, info};
use serde_json::{json, Value};

// Identity

pub async fn get_project_id() -> Result<String, anyhow::Error> {
    // TODO: read environment variable first and return early if set
    let shared_config = aws_config::from_env().load().await;
    let client = aws_sdk_sts::Client::new(&shared_config);

    let identity = client.get_caller_identity().send().await?;
    let account_id = identity.account().ok_or_else(|| anyhow::anyhow!("Account ID not found"))?;

    println!("Account ID: {}", account_id);

    Ok(account_id.to_string())
}

pub async fn get_user_id() -> Result<String, anyhow::Error> {
    let shared_config = aws_config::from_env().load().await;
    let client = aws_sdk_sts::Client::new(&shared_config);

    let identity = client.get_caller_identity().send().await?;
    let user_id = identity.arn().ok_or_else(|| anyhow::anyhow!("User ID not found"))?;

    info!("User ID: {}", user_id);

    Ok(user_id.to_string())
}

// This will be the only used function in the module

pub async fn run_function(payload: &Value) -> Result<GenericFunctionResponse, anyhow::Error> {
    let shared_config = aws_config::from_env().load().await;

    let client = Client::new(&shared_config);
    let api_function_name = "infraweave_api";
    let region_name = shared_config.region().expect("Region not set, did you forget to set AWS_REGION?");

    let serialized_payload =
        serde_json::to_vec(&payload).expect(&format!("Failed to serialize payload: {}", payload));

    let payload_blob = Blob::new(serialized_payload);

    let sanitized_payload = sanitize_payload_for_logging(payload.clone());
    info!(
        "Invoking generic job in region {} with payload: {}",
        region_name,
        serde_json::to_string_pretty(&sanitized_payload).unwrap(),
    );

    let request = client
        .invoke()
        .function_name(api_function_name)
        .invocation_type(InvocationType::RequestResponse)
        .payload(payload_blob);

    let response = match request.send().await {
        Ok(response) => response,
        Err(e) => {
            error!("Failed to invoke Lambda: {}\nAre you authenticated?", e);
            println!("Failed to invoke Lambda: {}\nAre you authenticated?", e);
            let error_message = format!("Failed to invoke Lambda: {}\nAre you authenticated?", e);
            return Err(anyhow::anyhow!(error_message));
        }
    };

    if let Some(blob) = response.payload {
        let bytes = blob.into_inner(); // Gets the Vec<u8>
        let response_string = String::from_utf8(bytes).expect("response not valid UTF-8");
        let parsed_json: Value =
            serde_json::from_str(&response_string).expect("response not valid JSON");
        info!("Lambda response: {}", serde_json::to_string_pretty(&parsed_json).unwrap());

        if parsed_json.get("errorType").is_some() {
            return Err(anyhow::anyhow!(
                "Error in Lambda response: {}",
                parsed_json.get("errorType").unwrap()
            ));
        }
        Ok(GenericFunctionResponse{ payload: parsed_json })
    } else {
        Err(anyhow::anyhow!("Payload missing from Lambda response"))
    }
}

pub async fn read_db(table: &str, query: &Value) -> Result<GenericFunctionResponse, anyhow::Error> {
    let full_query = json!({
        "event": "read_db",
        "table": table,
        "data": {
            "query": query
        }
    });
    run_function(&full_query).await
}

pub fn get_latest_module_version_query(module: &str, track: &str) -> Value {
    _get_latest_module_version_query("LATEST_MODULE", module, track)
}

pub fn get_latest_stack_version_query(stack: &str, track: &str) -> Value {
    _get_latest_module_version_query("LATEST_STACK", stack, track)
}

fn _get_latest_module_version_query(pk: &str, module: &str, track: &str) -> Value {
    let sk: String = format!(
        "MODULE#{}",
        get_module_identifier(&module, &track)
    );
    json!({
        "KeyConditionExpression": "PK = :latest AND SK = :sk",
        "ExpressionAttributeValues": {":latest": pk, ":sk": sk},
        "Limit": 1,
    })
}

pub fn get_all_latest_modules_query(track: &str) -> Value {
    _get_all_latest_modules_query("LATEST_MODULE", track)
}

pub fn get_all_latest_stacks_query(track: &str) -> Value {
    _get_all_latest_modules_query("LATEST_STACK", track)
}

fn _get_all_latest_modules_query(pk: &str, track: &str) -> Value {
    if track == "" {
        json!({        
            "KeyConditionExpression": "PK = :latest",
            "ExpressionAttributeValues": {":latest": pk},
        })    
    } else {
        json!({        
            "KeyConditionExpression": "PK = :latest and begins_with(SK, :track)",
            "ExpressionAttributeValues": {":latest": pk, ":track": format!("MODULE#{}::", track)},
        })
    }
}

pub fn get_all_module_versions_query(module: &str, track: &str) -> Value {
    _get_all_module_versions_query(module, track)
}

pub fn get_all_stack_versions_query(stack: &str, track: &str) -> Value {
    _get_all_module_versions_query(stack, track)
}

fn _get_all_module_versions_query(module: &str, track: &str) -> Value {
    let id: String = format!(
        "MODULE#{}",
        get_module_identifier(&module, &track)
    );
    json!({
        "KeyConditionExpression": "PK = :module AND begins_with(SK, :sk)",
        "ExpressionAttributeValues": {":module": id, ":sk": "VERSION#"},
        "ScanIndexForward": false,
    })
}

pub fn get_module_version_query(module: &str, track: &str, version: &str) -> Value {
    let id: String = format!("MODULE#{}", get_module_identifier(&module, &track));
    let version_id = format!("VERSION#{}", zero_pad_semver(version, 3).unwrap());
    json!({
        "KeyConditionExpression": "PK = :module AND SK = :sk",
        "ExpressionAttributeValues": {":module": id, ":sk": version_id},
        "Limit": 1,
    })
}

pub fn get_stack_version_query(module: &str, track: &str, version: &str) -> Value {
    get_module_version_query(module, track, version)
}

pub fn get_generate_presigned_url_query(key: &str, bucket: &str) -> Value {
    json!({
        "event": "generate_presigned_url",
            "data":{
            "key": key,
            "bucket_name": bucket,
            "expires_in": 60,
        }
    })
}

pub fn get_all_deployments_query(project_id: &str, region: &str, environment: &str) -> Value {
    json!({
        "IndexName": "DeletedIndex",
        "KeyConditionExpression": "deleted_PK_base = :deleted_PK_base AND begins_with(PK, :deployment_prefix)",
        "ExpressionAttributeValues": {
            ":deleted_PK_base": format!("0|DEPLOYMENT#{}", get_deployment_identifier(project_id, region, "",  "")),
            ":deployment_prefix": format!("DEPLOYMENT#{}", get_deployment_identifier(project_id, region, "",  environment)),
        }
    })
}

pub fn get_deployment_and_dependents_query(project_id: &str, region: &str, deployment_id: &str, environment: &str, include_deleted: bool) -> Value {
    json!({
        "KeyConditionExpression": "PK = :pk",
        "FilterExpression": "deleted <> :deleted",
        "ExpressionAttributeValues": {
            ":pk": format!("DEPLOYMENT#{}", get_deployment_identifier(project_id, region, deployment_id,  environment)),
            ":deleted": 1
        }
    })
}

pub fn get_deployment_query(project_id: &str, region: &str, deployment_id: &str, environment: &str, include_deleted: bool) -> Value {
    json!({
        "KeyConditionExpression": "PK = :pk AND SK = :metadata",
        "FilterExpression": "deleted = :deleted",
        "ExpressionAttributeValues": {
            ":pk": format!("DEPLOYMENT#{}", get_deployment_identifier(project_id, region, deployment_id, environment)),
            ":metadata": "METADATA",
            ":deleted": 0
        }
    })
}

pub fn get_deployments_using_module_query(project_id: &str, region: &str, module: &str, environment: &str) -> Value {
    let environment_refiner = if environment == "" { "" } else { 
        if environment.contains('/') { &format!("{}::", environment) } else { &format!("{}/", environment) }
    };
    json!({
        "IndexName": "ModuleIndex",
        "KeyConditionExpression": "#module = :module AND begins_with(deleted_PK, :deployment_prefix)",
        "ExpressionAttributeNames": {
            "#module": "module_PK_base"  // Aliasing the reserved keyword
        },
        "ExpressionAttributeValues": {
            ":deployment_prefix": format!("0|DEPLOYMENT#{}", get_deployment_identifier(project_id, region, "",  environment)),
            ":module": format!("MODULE#{}#{}", get_deployment_identifier(project_id, region, "",  ""), module),
            ":metadata": "METADATA"
        },
        "FilterExpression": "SK = :metadata", // Accepted as it results are few (only possibly additionl depedencies)
    })
}

pub fn get_plan_deployment_query(project_id: &str, region: &str, deployment_id: &str, environment: &str, job_id: &str) -> Value {
    json!({
        "KeyConditionExpression": "PK = :pk AND SK = :job_id",
        "FilterExpression": "deleted <> :deleted",
        "ExpressionAttributeValues": {
            ":pk": format!("PLAN#{}", get_deployment_identifier(project_id, region, deployment_id,  environment)),
            ":job_id": job_id,
            ":deleted": 1
        }
    })
}

pub fn get_dependents_query(project_id: &str, region: &str, deployment_id: &str, environment: &str) -> Value {
    json!({
        "KeyConditionExpression": "PK = :pk AND begins_with(SK, :dependent_prefix)",
        "FilterExpression": "deleted = :deleted",
        "ExpressionAttributeValues": {
            ":pk": format!("DEPLOYMENT#{}", get_deployment_identifier(project_id, region, deployment_id,  environment)),
            ":dependent_prefix": "DEPENDENT#",
            ":deleted": 0
        }
    })
}

pub fn get_deployments_to_driftcheck_query(project_id: &str, region: &str) -> Value {
    json!({
        "IndexName": "DriftCheckIndex",
        "KeyConditionExpression": "deleted_SK_base = :deleted_SK_base AND next_drift_check_epoch BETWEEN :start_epoch AND :current_epoch",
        "ExpressionAttributeValues": {
            ":deleted_SK_base": format!("0|METADATA#{}", get_deployment_identifier(project_id, region, "",  "")),
            ":start_epoch": 0,
            ":current_epoch": get_epoch()
        }
    })
}

pub fn get_all_projects_query() -> Value { // Only available using central role
    json!({
        "KeyConditionExpression": "PK = :PK",
        "ExpressionAttributeValues": {
            ":PK": "PROJECTS",
        }
    })
}

pub fn get_current_project_query(project_id: &str) -> Value {
    json!({
        "IndexName": "ReverseIndex",
        "KeyConditionExpression": "SK = :SK",
        "ExpressionAttributeValues": {
            ":SK": format!("PROJECT#{}", project_id),
        }
    })
}

// Event

pub fn get_events_query(project_id: &str, region: &str, deployment_id: &str, environment: &str) -> Value {
    json!({
        "KeyConditionExpression": "PK = :pk",
        "ExpressionAttributeValues": {":pk": format!("EVENT#{}", get_event_identifier(project_id, region, deployment_id, environment))}
    })
}

pub fn get_all_events_between_query(region: &str, start_epoch: u128, end_epoch: u128) -> Value {
    json!({
        "IndexName": "RegionIndex",
        "KeyConditionExpression": "PK_base_region = :pk_base_region AND SK BETWEEN :start_epoch AND :end_epoch",
        "ExpressionAttributeValues": {
            ":pk_base_region": format!("EVENT#{}", region),
            ":start_epoch": start_epoch.to_string(),
            ":end_epoch": end_epoch.to_string(),
        }
    })
}

// Change record

pub fn get_change_records_query(project_id: &str, region: &str, environment: &str, deployment_id: &str, job_id: &str, change_type: &str) -> Value {
    json!({
        "KeyConditionExpression": "PK = :pk AND SK = :sk",
        "ExpressionAttributeValues": {
            ":pk": format!("{}#{}", change_type, get_change_record_identifier(project_id, region, deployment_id, environment)),
            ":sk": job_id
        }
    })
}

// Policy

pub fn get_newest_policy_version_query(policy: &str, environment: &str) -> Value {
    json!({
        "KeyConditionExpression": "PK = :policy",
        "ExpressionAttributeValues": {":policy": format!("POLICY#{}", get_policy_identifier(&policy, &environment))},
        "ScanIndexForward": false,
        "Limit": 1,
    })
}

pub fn get_all_policies_query(environment: &str) -> Value {
    json!({
        "KeyConditionExpression": "PK = :current AND begins_with(SK, :policy_prefix)",
        "ExpressionAttributeValues": {":current": "CURRENT", ":policy_prefix": format!("POLICY#{}", environment)},
    })
}

pub fn get_policy_query(policy: &str, environment: &str, version: &str) -> Value {
    json!({
        "KeyConditionExpression": "PK = :policy AND SK = :version",
        "ExpressionAttributeValues": {
            ":policy": format!("POLICY#{}", get_policy_identifier(&policy, &environment)),
            ":version": format!("VERSION#{}", zero_pad_semver(&version, 3).unwrap())
        },
        "Limit": 1,
    })
}