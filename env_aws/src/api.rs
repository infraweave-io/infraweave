use aws_sdk_lambda::primitives::Blob;
use aws_sdk_lambda::types::InvocationType;
use aws_sdk_sts::types::Credentials;
use env_defs::{
    get_change_record_identifier, get_deployment_identifier, get_event_identifier,
    get_module_identifier, get_policy_identifier, CloudHandlerError, GenericFunctionResponse,
};
use env_utils::{get_epoch, sanitize_payload_for_logging, zero_pad_semver};
use log::{error, info};
use serde_json::{json, Value};

// Identity

pub async fn get_project_id() -> Result<String, anyhow::Error> {
    // TODO: read environment variable first and return early if set
    let shared_config = aws_config::from_env().load().await;
    let client = aws_sdk_sts::Client::new(&shared_config);

    let identity = client.get_caller_identity().send().await?;
    let account_id = identity
        .account()
        .ok_or_else(|| anyhow::anyhow!("Account ID not found"))?;

    Ok(account_id.to_string())
}

pub async fn get_user_id() -> Result<String, anyhow::Error> {
    if std::env::var("TEST_MODE").is_ok() {
        return Ok("test_user_id".to_string());
    };
    let shared_config = aws_config::from_env().load().await;
    let client = aws_sdk_sts::Client::new(&shared_config);

    let identity = client.get_caller_identity().send().await?;
    let user_id = identity
        .arn()
        .ok_or_else(|| anyhow::anyhow!("User ID not found"))?;

    Ok(user_id.to_string())
}

pub async fn assume_role(
    role_arn: &str,
    session_name: &str,
    duration_seconds: i32,
) -> Result<Credentials, anyhow::Error> {
    let shared_config = aws_config::from_env().load().await;
    let client = aws_sdk_sts::Client::new(&shared_config);

    let resp = client
        .assume_role()
        .role_arn(role_arn)
        .role_session_name(session_name)
        .duration_seconds(duration_seconds)
        .send()
        .await?;

    let creds = resp.credentials().unwrap().clone();

    Ok(creds)
}

// This will be the only used function in the module
pub async fn get_lambda_client(
    lambda_endpoint_url: Option<String>,
    project_id: &str,
    region: &str,
) -> (aws_sdk_lambda::Client, String) {
    let shared_config = aws_config::from_env().load().await;
    match std::env::var("TEST_MODE") {
        Ok(_) => {
            let test_region = "us-west-2";
            let lambda_endpoint_url =
                lambda_endpoint_url.expect("lambda_endpoint_url variable not set");
            let test_lambda_config = aws_sdk_lambda::config::Builder::from(&shared_config)
                .endpoint_url(lambda_endpoint_url)
                .region(aws_config::Region::new(test_region))
                .build();
            (
                aws_sdk_lambda::Client::from_conf(test_lambda_config),
                test_region.to_string(),
            )
        }
        Err(_) => {
            info!("Using project_id: {} and region: {}", project_id, region);
            let prod_lambda_config = aws_sdk_lambda::config::Builder::from(&shared_config)
                .region(aws_sdk_lambda::config::Region::new(region.to_string()))
                .build();
            (
                aws_sdk_lambda::Client::from_conf(prod_lambda_config),
                region.to_string(),
            )
        }
    }
}

pub async fn run_function(
    function_endpoint: &Option<String>,
    payload: &Value,
    project_id: &str,
    region: &str,
) -> Result<GenericFunctionResponse, CloudHandlerError> {
    let (client, _region_name) =
        get_lambda_client(function_endpoint.clone(), project_id, region).await;
    let api_environment = match std::env::var("INFRAWEAVE_ENV") {
        Ok(env) => env,
        Err(_) => {
            // println!("Please make sure to set the platform environment, for example: \"export INFRAWEAVE_ENV=some-env\"");
            // println!("Defaulting to 'prod' environment");
            "prod".to_string()
        }
    };

    let serialized_payload = serde_json::to_vec(&payload)
        .unwrap_or_else(|_| panic!("Failed to serialize payload: {}", payload));

    let payload_blob = Blob::new(serialized_payload);

    let sanitized_payload = sanitize_payload_for_logging(payload.clone());
    if std::env::var("TEST_MODE").is_ok() {
        let payload_event = payload.get("event").unwrap_or(&Value::Null);
        eprintln!(
            "Running {} function in test mode ({})",
            payload_event.as_str().unwrap_or("No event specified"),
            function_endpoint
                .as_deref()
                .unwrap_or("No endpoint specified")
        );
    }
    info!(
        "Invoking generic job in region {} with payload: {}",
        _region_name,
        serde_json::to_string(&sanitized_payload).unwrap(),
    );

    let api_function_name = match std::env::var("INFRAWEAVE_API_FUNCTION") {
        Ok(name) => {
            info!(
                "Using custom function name from INFRAWEAVE_API_FUNCTION: {}",
                &name
            );
            name
        }
        Err(_) => format!("infraweave-api-{}", api_environment),
    };

    let function_name = match std::env::var("TEST_MODE") {
        Ok(_) => api_function_name,
        Err(_) => format!("{}:function:{}", project_id, api_function_name),
    };

    let request = client
        .invoke()
        .function_name(function_name)
        .invocation_type(InvocationType::RequestResponse)
        .payload(payload_blob);

    let response = match request.send().await {
        Ok(response) => response,
        Err(e) => {
            error!("Failed to invoke Lambda: {}\nAre you authenticated?", e);
            eprintln!("Failed to invoke Lambda: {}\nAre you authenticated?", e);
            let error_message = format!("Failed to invoke Lambda: {}\nAre you authenticated?", e);
            return Err(CloudHandlerError::Unauthenticated(error_message));
        }
    };

    if let Some(blob) = response.payload {
        let bytes = blob.into_inner(); // Gets the Vec<u8>
        let response_string = String::from_utf8(bytes).expect("response not valid UTF-8");
        let parsed_json: Value =
            serde_json::from_str(&response_string).expect("response not valid JSON");
        info!(
            "Lambda response: {}",
            serde_json::to_string(&parsed_json).unwrap()
        );

        if parsed_json.get("errorType").is_some() {
            match parsed_json.get("errorType").unwrap().as_str().unwrap() {
                "Unauthenticated" => {
                    return Err(CloudHandlerError::Unauthenticated(
                        parsed_json.get("errorMessage").unwrap().to_string(),
                    ));
                }
                "IndexError" => {
                    return Err(CloudHandlerError::NoAvailableRunner());
                }
                _ => {
                    return Err(CloudHandlerError::OtherError(format!(
                        "Error in Lambda response: {}",
                        parsed_json.get("errorMessage").unwrap()
                    )));
                }
            }
        }
        Ok(GenericFunctionResponse {
            payload: parsed_json,
        })
    } else {
        Err(CloudHandlerError::MissingPayload())
    }
}

pub async fn read_db(
    function_endpoint: &Option<String>,
    table: &str,
    query: &Value,
    project_id: &str,
    region: &str,
) -> Result<GenericFunctionResponse, CloudHandlerError> {
    let full_query = json!({
        "event": "read_db",
        "table": table,
        "data": {
            "query": query
        }
    });
    run_function(function_endpoint, &full_query, project_id, region).await
}

pub fn get_latest_module_version_query(module: &str, track: &str) -> Value {
    _get_latest_module_version_query("LATEST_MODULE", module, track)
}

pub fn get_latest_stack_version_query(stack: &str, track: &str) -> Value {
    _get_latest_module_version_query("LATEST_STACK", stack, track)
}

pub fn get_latest_provider_version_query(provider: &str) -> Value {
    _get_latest_provider_version_query("LATEST_PROVIDER", provider)
}

fn _get_latest_module_version_query(pk: &str, module: &str, track: &str) -> Value {
    let sk: String = format!("MODULE#{}", get_module_identifier(module, track));
    json!({
        "KeyConditionExpression": "PK = :latest AND SK = :sk",
        "ExpressionAttributeValues": {":latest": pk, ":sk": sk},
        "Limit": 1,
    })
}

fn _get_latest_provider_version_query(pk: &str, provider: &str) -> Value {
    let sk: String = format!("PROVIDER#{}", provider);
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

pub fn get_all_latest_providers_query() -> Value {
    _get_all_latest_providers_query("LATEST_PROVIDER")
}

fn _get_all_latest_modules_query(pk: &str, track: &str) -> Value {
    if track.is_empty() {
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

fn _get_all_latest_providers_query(pk: &str) -> Value {
    json!({
        "KeyConditionExpression": "PK = :latest",
        "ExpressionAttributeValues": {":latest": pk},
    })
}

pub fn get_all_module_versions_query(module: &str, track: &str) -> Value {
    _get_all_module_versions_query(module, track)
}

pub fn get_all_stack_versions_query(stack: &str, track: &str) -> Value {
    _get_all_module_versions_query(stack, track)
}

fn _get_all_module_versions_query(module: &str, track: &str) -> Value {
    let id: String = format!("MODULE#{}", get_module_identifier(module, track));
    json!({
        "KeyConditionExpression": "PK = :module AND begins_with(SK, :sk)",
        "ExpressionAttributeValues": {":module": id, ":sk": "VERSION#"},
        "ScanIndexForward": false,
    })
}

pub fn get_module_version_query(module: &str, track: &str, version: &str) -> Value {
    let id: String = format!("MODULE#{}", get_module_identifier(module, track));
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

pub fn get_job_status_query(job_id: &str) -> Value {
    json!({
        "event": "get_job_status",
        "data": {
            "job_id": job_id
        }
    })
}

pub fn get_environment_variables_query() -> Value {
    json!({
        "event": "get_environment_variables"
    })
}

pub fn get_all_deployments_query(
    project_id: &str,
    region: &str,
    environment: &str,
    include_deleted: bool,
) -> Value {
    if include_deleted {
        json!({
            "KeyConditionExpression": "begins_with(PK, :deployment_prefix)",
            "FilterExpression": "SK = :metadata",
            "ExpressionAttributeValues": {
                ":deployment_prefix": format!("DEPLOYMENT#{}", get_deployment_identifier(project_id, region, "",  environment)),
                ":metadata": "METADATA"
            }
        })
    } else {
        json!({
            "IndexName": "DeletedIndex",
            "KeyConditionExpression": "deleted_PK_base = :deleted_PK_base AND begins_with(PK, :deployment_prefix)",
            "ExpressionAttributeValues": {
                ":deleted_PK_base": format!("0|DEPLOYMENT#{}", get_deployment_identifier(project_id, region, "",  "")),
                ":deployment_prefix": format!("DEPLOYMENT#{}", get_deployment_identifier(project_id, region, "",  environment)),
            }
        })
    }
}

pub fn get_deployment_and_dependents_query(
    project_id: &str,
    region: &str,
    deployment_id: &str,
    environment: &str,
    include_deleted: bool,
) -> Value {
    if include_deleted {
        json!({
            "KeyConditionExpression": "PK = :pk",
            "ExpressionAttributeValues": {
                ":pk": format!("DEPLOYMENT#{}", get_deployment_identifier(project_id, region, deployment_id,  environment))
            }
        })
    } else {
        json!({
            "KeyConditionExpression": "PK = :pk",
            "FilterExpression": "deleted <> :deleted",
            "ExpressionAttributeValues": {
                ":pk": format!("DEPLOYMENT#{}", get_deployment_identifier(project_id, region, deployment_id,  environment)),
                ":deleted": 1
            }
        })
    }
}

pub fn get_deployment_query(
    project_id: &str,
    region: &str,
    deployment_id: &str,
    environment: &str,
    include_deleted: bool,
) -> Value {
    if include_deleted {
        json!({
            "KeyConditionExpression": "PK = :pk AND SK = :metadata",
            "ExpressionAttributeValues": {
                ":pk": format!("DEPLOYMENT#{}", get_deployment_identifier(project_id, region, deployment_id, environment)),
                ":metadata": "METADATA"
            }
        })
    } else {
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
}

// TODO: use environment_refiner
pub fn get_deployments_using_module_query(
    project_id: &str,
    region: &str,
    module: &str,
    environment: &str,
    include_deleted: bool,
) -> Value {
    let _environment_refiner = if environment.is_empty() {
        ""
    } else if environment.contains('/') {
        &format!("{}::", environment)
    } else {
        &format!("{}/", environment)
    };

    if include_deleted {
        json!({
            "IndexName": "ModuleIndex",
            "KeyConditionExpression": "#module = :module AND begins_with(deleted_PK, :deployment_prefix)",
            "ExpressionAttributeNames": {
                "#module": "module_PK_base"  // Aliasing the reserved keyword
            },
            "ExpressionAttributeValues": {
                ":deployment_prefix": format!("DEPLOYMENT#{}", get_deployment_identifier(project_id, region, "",  environment)),
                ":module": format!("MODULE#{}#{}", get_deployment_identifier(project_id, region, "",  ""), module),
                ":metadata": "METADATA"
            },
            "FilterExpression": "SK = :metadata", // Accepted as it results are few (only possibly additionl depedencies)
        })
    } else {
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
}

pub fn get_plan_deployment_query(
    project_id: &str,
    region: &str,
    deployment_id: &str,
    environment: &str,
    job_id: &str,
) -> Value {
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

pub fn get_dependents_query(
    project_id: &str,
    region: &str,
    deployment_id: &str,
    environment: &str,
) -> Value {
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

pub fn get_all_projects_query() -> Value {
    // Only available using central role
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

pub fn get_events_query(
    project_id: &str,
    region: &str,
    deployment_id: &str,
    environment: &str,
) -> Value {
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

pub fn get_change_records_query(
    project_id: &str,
    region: &str,
    environment: &str,
    deployment_id: &str,
    job_id: &str,
    change_type: &str,
) -> Value {
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
        "ExpressionAttributeValues": {":policy": format!("POLICY#{}", get_policy_identifier(policy, environment))},
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
            ":policy": format!("POLICY#{}", get_policy_identifier(policy, environment)),
            ":version": format!("VERSION#{}", zero_pad_semver(version, 3).unwrap())
        },
        "Limit": 1,
    })
}

pub fn get_project_map_query() -> Value {
    json!({
        "KeyConditionExpression": "PK = :project_map",
        "ExpressionAttributeValues": {
            ":project_map": "project_map",
        },
        "Limit": 1,
    })
}

pub fn get_all_regions_query() -> Value {
    json!({
        "KeyConditionExpression": "PK = :all_regions",
        "ExpressionAttributeValues": {
            ":all_regions": "all_regions",
        },
        "Limit": 1,
    })
}
