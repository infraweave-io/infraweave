use aws_sdk_sts::types::Credentials;
use env_defs::{
    get_change_record_identifier, get_deployment_identifier, get_event_identifier,
    get_module_identifier, get_policy_identifier, CloudHandlerError, GenericFunctionResponse,
};
use env_utils::{get_epoch, zero_pad_semver};
use serde_json::{json, Value};

use crate::is_http_mode_enabled;

#[cfg(not(feature = "direct"))]
use aws_sdk_lambda::primitives::Blob;
#[cfg(not(feature = "direct"))]
use aws_sdk_lambda::types::InvocationType;
#[cfg(not(feature = "direct"))]
use env_utils::sanitize_payload_for_logging;
#[cfg(not(feature = "direct"))]
use log::{error, info};

#[cfg(feature = "direct")]
use log::info;

// Identity

pub async fn get_project_id() -> Result<String, anyhow::Error> {
    // In HTTP API mode, return dummy account ID without AWS SDK calls
    if crate::is_http_mode_enabled() {
        return Ok("000000000000".to_string());
    }

    if let Ok(account_id) = std::env::var("ACCOUNT_ID") {
        return Ok(account_id);
    }

    #[cfg(feature = "direct")]
    {
        // Local mode - return dummy account ID
        return Ok("000000000000".to_string());
    }

    #[cfg(not(feature = "direct"))]
    {
        let shared_config = aws_config::from_env().load().await;
        let client = aws_sdk_sts::Client::new(&shared_config);

        let result = client
            .get_caller_identity()
            .send()
            .await
            .map_err(|e| anyhow::anyhow!("Failed to get caller identity: {}", e))?;

        Ok(result
            .account()
            .ok_or_else(|| anyhow::anyhow!("Account ID not found"))?
            .to_string())
    }
}

pub async fn get_user_id() -> Result<String, anyhow::Error> {
    if std::env::var("TEST_MODE").is_ok() {
        return Ok("test_user_id".to_string());
    };

    // HTTP mode - return placeholder, server will extract real user from JWT
    if is_http_mode_enabled() {
        return Ok("http-mode-user".to_string());
    }

    #[cfg(feature = "direct")]
    {
        // Local mode - return dummy user ID
        return Ok("arn:aws:iam::000000000000:user/local-user".to_string());
    }

    #[cfg(not(feature = "direct"))]
    {
        let shared_config = aws_config::from_env().load().await;
        let client = aws_sdk_sts::Client::new(&shared_config);

        let identity = client.get_caller_identity().send().await?;
        let user_id = identity
            .arn()
            .ok_or_else(|| anyhow::anyhow!("User ID not found"))?;

        Ok(user_id.to_string())
    }
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

#[allow(dead_code)]
pub async fn get_lambda_client(
    lambda_endpoint_url: Option<String>,
    project_id: &str,
    region: &str,
) -> (aws_sdk_lambda::Client, String) {
    let region_provider = aws_config::Region::new(region.to_string());
    let shared_config = aws_config::from_env().region(region_provider).load().await;
    match std::env::var("TEST_MODE") {
        Ok(_) => {
            let test_region = std::env::var("AWS_REGION")
                .or_else(|_| std::env::var("TEST_REGION"))
                .expect("AWS_REGION or TEST_REGION must be set in TEST_MODE");
            let lambda_endpoint_url =
                lambda_endpoint_url.expect("lambda_endpoint_url variable not set");
            let test_lambda_config = aws_sdk_lambda::config::Builder::from(&shared_config)
                .endpoint_url(lambda_endpoint_url)
                .region(aws_config::Region::new(test_region.clone()))
                .build();
            (
                aws_sdk_lambda::Client::from_conf(test_lambda_config),
                test_region,
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

#[allow(dead_code)]
fn get_s3_client(config: &aws_config::SdkConfig) -> aws_sdk_s3::Client {
    let mut builder = aws_sdk_s3::config::Builder::from(config);
    if std::env::var("AWS_S3_FORCE_PATH_STYLE")
        .map(|v| v == "true")
        .unwrap_or(false)
    {
        builder = builder.force_path_style(true);
    }
    aws_sdk_s3::Client::from_conf(builder.build())
}

#[cfg(feature = "direct")]
async fn get_s3_client_direct(region: &str) -> aws_sdk_s3::Client {
    use aws_sdk_s3::config::{BehaviorVersion, Credentials, Region};

    // Check for MinIO/custom S3 endpoint (local development)
    let endpoint_opt = std::env::var("AWS_ENDPOINT_URL_S3")
        .or_else(|_| std::env::var("MINIO_ENDPOINT"))
        .ok();

    if let Some(endpoint) = endpoint_opt {
        // Local development mode - use MinIO/custom endpoint
        eprintln!("Local mode: Using S3 endpoint {}", endpoint);

        let credentials = Credentials::new(
            std::env::var("AWS_ACCESS_KEY_ID").unwrap_or_else(|_| "minio".to_string()),
            std::env::var("AWS_SECRET_ACCESS_KEY").unwrap_or_else(|_| "minio123".to_string()),
            None,
            None,
            "local",
        );

        let config_builder = aws_sdk_s3::Config::builder()
            .behavior_version(BehaviorVersion::latest())
            .credentials_provider(credentials)
            .region(Region::new(region.to_string()))
            .force_path_style(true)
            .endpoint_url(endpoint);

        aws_sdk_s3::Client::from_conf(config_builder.build())
    } else {
        // Production mode - use real AWS S3 with specified region
        let config = aws_config::from_env()
            .region(aws_config::Region::new(region.to_string()))
            .load()
            .await;
        get_s3_client(&config)
    }
}

#[cfg(feature = "direct")]
pub async fn run_function(
    _function_endpoint: &Option<String>,
    payload: &Value,
    project_id: &str,
    region: &str,
) -> Result<GenericFunctionResponse, CloudHandlerError> {
    use crate::direct_impl::{
        get_environment_variables_direct, get_job_status_direct, insert_db_direct, read_db_direct,
        read_logs_direct, transact_write_direct,
    };
    use crate::utils::get_bucket_name_from_env;
    use aws_sdk_s3::primitives::ByteStream;
    use base64::Engine;

    let event = payload
        .get("event")
        .and_then(|e| e.as_str())
        .ok_or_else(|| CloudHandlerError::OtherError("Missing event field".to_string()))?;

    match event {
        "read_db" => {
            let table = payload
                .get("table")
                .and_then(|t| t.as_str())
                .ok_or_else(|| CloudHandlerError::OtherError("Missing table field".to_string()))?;
            let query = payload
                .get("data")
                .and_then(|d| d.get("query"))
                .ok_or_else(|| CloudHandlerError::OtherError("Missing query field".to_string()))?;

            match read_db_direct(table, query, Some(region)).await {
                Ok(data) => Ok(GenericFunctionResponse { payload: data }),
                Err(e) => Err(CloudHandlerError::OtherError(format!(
                    "Direct DB read failed: {}",
                    e
                ))),
            }
        }
        "get_job_status" => {
            let job_id = payload
                .get("data")
                .and_then(|d| d.get("job_id"))
                .and_then(|j| j.as_str())
                .ok_or_else(|| CloudHandlerError::OtherError("Missing job_id field".to_string()))?;

            match get_job_status_direct(job_id, Some(region)).await {
                Ok(data) => Ok(GenericFunctionResponse { payload: data }),
                Err(e) => Err(CloudHandlerError::OtherError(format!(
                    "Direct get_job_status failed: {}",
                    e
                ))),
            }
        }
        "read_logs" => {
            let data = payload
                .get("data")
                .ok_or_else(|| CloudHandlerError::OtherError("Missing data field".to_string()))?;
            let job_id = data
                .get("job_id")
                .and_then(|j| j.as_str())
                .ok_or_else(|| CloudHandlerError::OtherError("Missing job_id field".to_string()))?;
            let next_token = data.get("next_token").and_then(|t| t.as_str());
            let limit = data.get("limit").and_then(|l| l.as_i64()).map(|l| l as i32);

            match read_logs_direct(job_id, project_id, region, next_token, limit).await {
                Ok(data) => Ok(GenericFunctionResponse { payload: data }),
                Err(e) => Err(CloudHandlerError::OtherError(format!(
                    "Direct read_logs failed: {}",
                    e
                ))),
            }
        }
        "get_environment_variables" => match get_environment_variables_direct() {
            Ok(data) => Ok(GenericFunctionResponse { payload: data }),
            Err(e) => Err(CloudHandlerError::OtherError(format!(
                "Direct get_environment_variables failed: {}",
                e
            ))),
        },
        "generate_presigned_url" => {
            use aws_sdk_s3::presigning::PresigningConfig;
            use std::time::Duration;

            let data = payload
                .get("data")
                .ok_or_else(|| CloudHandlerError::OtherError("Missing data field".to_string()))?;
            let key = data
                .get("key")
                .and_then(|k| k.as_str())
                .ok_or_else(|| CloudHandlerError::OtherError("Missing key field".to_string()))?;
            let bucket = data
                .get("bucket_name")
                .and_then(|b| b.as_str())
                .ok_or_else(|| {
                    CloudHandlerError::OtherError("Missing bucket_name field".to_string())
                })?;

            let actual_bucket =
                get_bucket_name_from_env(bucket, region).unwrap_or_else(|| bucket.to_string());

            let client = get_s3_client_direct(region).await;

            let presigning_config = PresigningConfig::builder()
                .expires_in(Duration::from_secs(60))
                .build()
                .map_err(|e| {
                    CloudHandlerError::OtherError(format!(
                        "Failed to build presigning config: {}",
                        e
                    ))
                })?;

            let presigned_request = client
                .get_object()
                .bucket(&actual_bucket)
                .key(key)
                .presigned(presigning_config)
                .await
                .map_err(|e| {
                    CloudHandlerError::OtherError(format!(
                        "Failed to generate presigned URL: {}",
                        e
                    ))
                })?;

            let url = presigned_request.uri().to_string();
            Ok(GenericFunctionResponse {
                payload: json!({ "url": url }),
            })
        }
        "upload_file_base64" => {
            let data = payload
                .get("data")
                .ok_or_else(|| CloudHandlerError::OtherError("Missing data field".to_string()))?;
            let key = data
                .get("key")
                .and_then(|k| k.as_str())
                .ok_or_else(|| CloudHandlerError::OtherError("Missing key field".to_string()))?;
            let bucket = data
                .get("bucket_name")
                .and_then(|b| b.as_str())
                .ok_or_else(|| {
                    CloudHandlerError::OtherError("Missing bucket_name field".to_string())
                })?;
            let base64_content = data
                .get("base64_content")
                .and_then(|c| c.as_str())
                .ok_or_else(|| {
                    CloudHandlerError::OtherError("Missing base64_content field".to_string())
                })?;

            let actual_bucket =
                get_bucket_name_from_env(bucket, region).unwrap_or_else(|| bucket.to_string());
            log::info!(
                "Uploading {} to {}/{} (region: {})",
                key,
                actual_bucket,
                bucket,
                region
            );

            let bytes = base64::engine::general_purpose::STANDARD
                .decode(base64_content)
                .map_err(|e| {
                    CloudHandlerError::OtherError(format!("Failed to decode base64: {}", e))
                })?;

            let client = get_s3_client_direct(region).await;

            client
                .put_object()
                .bucket(&actual_bucket)
                .key(key)
                .body(ByteStream::from(bytes))
                .send()
                .await
                .map_err(|e| {
                    log::error!("Failed to upload {} to S3: {:?}", key, e);
                    CloudHandlerError::OtherError(format!("Failed to upload to S3: {:?}", e))
                })?;

            log::info!("Successfully uploaded {} to S3", key);
            Ok(GenericFunctionResponse {
                payload: json!({ "success": true, "key": key, "bucket": actual_bucket }),
            })
        }
        "upload_file_url" => {
            let data = payload
                .get("data")
                .ok_or_else(|| CloudHandlerError::OtherError("Missing data field".to_string()))?;
            let key = data
                .get("key")
                .and_then(|k| k.as_str())
                .ok_or_else(|| CloudHandlerError::OtherError("Missing key field".to_string()))?;
            let bucket = data
                .get("bucket_name")
                .and_then(|b| b.as_str())
                .ok_or_else(|| {
                    CloudHandlerError::OtherError("Missing bucket_name field".to_string())
                })?;
            let url = data
                .get("url")
                .and_then(|u| u.as_str())
                .ok_or_else(|| CloudHandlerError::OtherError("Missing url field".to_string()))?;

            let actual_bucket =
                get_bucket_name_from_env(bucket, region).unwrap_or_else(|| bucket.to_string());
            log::info!(
                "Downloading from {} and uploading to {}/{}",
                url,
                actual_bucket,
                key
            );

            let response = reqwest::get(url).await.map_err(|e| {
                CloudHandlerError::OtherError(format!("Failed to download from URL: {}", e))
            })?;

            let bytes = response.bytes().await.map_err(|e| {
                CloudHandlerError::OtherError(format!("Failed to read response bytes: {}", e))
            })?;

            let client = get_s3_client_direct(region).await;

            client
                .put_object()
                .bucket(&actual_bucket)
                .key(key)
                .body(ByteStream::from(bytes.to_vec()))
                .send()
                .await
                .map_err(|e| {
                    log::error!("Failed to upload {} to S3: {:?}", key, e);
                    CloudHandlerError::OtherError(format!("Failed to upload to S3: {:?}", e))
                })?;

            log::info!("Successfully uploaded {} to S3", key);
            Ok(GenericFunctionResponse {
                payload: json!({ "success": true, "key": key, "bucket": actual_bucket }),
            })
        }
        "download_file" => {
            let data = payload
                .get("data")
                .ok_or_else(|| CloudHandlerError::OtherError("Missing data field".to_string()))?;
            let key = data
                .get("key")
                .and_then(|k| k.as_str())
                .ok_or_else(|| CloudHandlerError::OtherError("Missing key field".to_string()))?;
            let bucket = data
                .get("bucket_name")
                .and_then(|b| b.as_str())
                .ok_or_else(|| {
                    CloudHandlerError::OtherError("Missing bucket_name field".to_string())
                })?;

            let actual_bucket =
                get_bucket_name_from_env(bucket, region).unwrap_or_else(|| bucket.to_string());

            log::info!(
                "Downloading {} from bucket {} (resolved to {})",
                key,
                bucket,
                actual_bucket
            );

            let client = get_s3_client_direct(region).await;

            match client
                .get_object()
                .bucket(&actual_bucket)
                .key(key)
                .send()
                .await
            {
                Ok(response) => {
                    let bytes = response
                        .body
                        .collect()
                        .await
                        .map_err(|e| {
                            CloudHandlerError::OtherError(format!(
                                "Failed to read S3 object body: {}",
                                e
                            ))
                        })?
                        .into_bytes();

                    let base64_content =
                        base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &bytes);

                    Ok(GenericFunctionResponse {
                        payload: json!({ "content": base64_content }),
                    })
                }
                Err(e) => {
                    log::error!("Failed to download {}/{}: {:?}", actual_bucket, key, e);
                    Err(CloudHandlerError::OtherError(format!(
                        "Failed to download from S3: {}",
                        e
                    )))
                }
            }
        }
        "transact_write" => {
            let items = payload
                .get("items")
                .ok_or_else(|| CloudHandlerError::OtherError("Missing items field".to_string()))?;

            match transact_write_direct(items, Some(region)).await {
                Ok(data) => Ok(GenericFunctionResponse { payload: data }),
                Err(e) => Err(CloudHandlerError::OtherError(format!(
                    "Direct transact_write failed: {}",
                    e
                ))),
            }
        }
        "insert_db" => {
            let table = payload
                .get("table")
                .and_then(|t| t.as_str())
                .ok_or_else(|| CloudHandlerError::OtherError("Missing table field".to_string()))?;
            let data = payload
                .get("data")
                .ok_or_else(|| CloudHandlerError::OtherError("Missing data field".to_string()))?;

            match insert_db_direct(table, data, Some(region)).await {
                Ok(data) => Ok(GenericFunctionResponse { payload: data }),
                Err(e) => Err(CloudHandlerError::OtherError(format!(
                    "Direct insert_db failed: {}",
                    e
                ))),
            }
        }
        _ => Err(CloudHandlerError::OtherError(format!(
            "Unknown event type: {}",
            event
        ))),
    }
}

// Lambda invocation implementation - calls internal Lambda API
#[cfg(not(feature = "direct"))]
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
    let full_query = env_defs::read_db_event(table, query);
    run_function(function_endpoint, &full_query, project_id, region).await
}

#[allow(dead_code)]
pub async fn start_runner(event: &Value) -> Result<Value, anyhow::Error> {
    let payload = event
        .get("data")
        .ok_or_else(|| anyhow::anyhow!("Missing data"))?;

    let shared_config = aws_config::from_env().load().await;
    let client = aws_sdk_ecs::Client::new(&shared_config);

    let ecs_cluster_name = std::env::var("ECS_CLUSTER_NAME")
        .map_err(|_| anyhow::anyhow!("ECS_CLUSTER_NAME not set"))?;
    let ecs_task_definition = std::env::var("ECS_TASK_DEFINITION")
        .map_err(|_| anyhow::anyhow!("ECS_TASK_DEFINITION not set"))?;

    let subnet_id = std::env::var("SUBNET_ID").map_err(|_| anyhow::anyhow!("SUBNET_ID not set"))?;
    let security_group_id = std::env::var("SECURITY_GROUP_ID")
        .map_err(|_| anyhow::anyhow!("SECURITY_GROUP_ID not set"))?;

    let cpu = payload.get("cpu").and_then(|v| v.as_str()).unwrap_or("256");
    let memory = payload
        .get("memory")
        .and_then(|v| v.as_str())
        .unwrap_or("512");

    let res = client
        .run_task()
        .cluster(ecs_cluster_name)
        .task_definition(ecs_task_definition)
        .launch_type(aws_sdk_ecs::types::LaunchType::Fargate)
        .overrides(
            aws_sdk_ecs::types::TaskOverride::builder()
                .cpu(cpu)
                .memory(memory)
                .container_overrides(
                    aws_sdk_ecs::types::ContainerOverride::builder()
                        .name("runner")
                        .cpu(cpu.parse::<i32>()?)
                        .memory(memory.parse::<i32>()?)
                        .environment(
                            aws_sdk_ecs::types::KeyValuePair::builder()
                                .name("PAYLOAD")
                                .value(serde_json::to_string(payload)?)
                                .build(),
                        )
                        .build(),
                )
                .build(),
        )
        .network_configuration(
            aws_sdk_ecs::types::NetworkConfiguration::builder()
                .awsvpc_configuration(
                    aws_sdk_ecs::types::AwsVpcConfiguration::builder()
                        .subnets(subnet_id)
                        .security_groups(security_group_id)
                        .assign_public_ip(aws_sdk_ecs::types::AssignPublicIp::Enabled)
                        .build()
                        .map_err(|e| anyhow::anyhow!(e))?,
                )
                .build(),
        )
        .count(1)
        .send()
        .await?;

    println!("res: {:?}", res);

    let job_id = res
        .tasks
        .as_ref()
        .and_then(|t| t.first())
        .and_then(|t| t.task_arn.as_ref())
        .map(|arn| arn.split('/').last().unwrap_or(arn))
        .ok_or_else(|| anyhow::anyhow!("Failed to retrieve task details"))?;

    Ok(json!({ "job_id": job_id }))
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

pub fn get_all_latest_modules_query(
    track: &str,
    include_deprecated: bool,
    include_dev000: bool,
) -> Value {
    _get_all_latest_modules_query("LATEST_MODULE", track, include_deprecated, include_dev000)
}

pub fn get_all_latest_stacks_query(
    track: &str,
    include_deprecated: bool,
    include_dev000: bool,
) -> Value {
    _get_all_latest_modules_query("LATEST_STACK", track, include_deprecated, include_dev000)
}

pub fn get_all_latest_providers_query() -> Value {
    _get_all_latest_providers_query("LATEST_PROVIDER")
}

fn _get_all_latest_modules_query(
    pk: &str,
    track: &str,
    include_deprecated: bool,
    include_dev000: bool,
) -> Value {
    let mut query = if track.is_empty() {
        json!({
            "KeyConditionExpression": "PK = :latest",
            "ExpressionAttributeValues": {":latest": pk},
        })
    } else {
        json!({
            "KeyConditionExpression": "PK = :latest and begins_with(SK, :track)",
            "ExpressionAttributeValues": {":latest": pk, ":track": format!("MODULE#{}::", track)},
        })
    };

    let mut filters = Vec::new();

    if !include_deprecated {
        filters.push("attribute_not_exists(deprecated) OR deprecated = :false");
    }

    if !include_dev000 {
        filters.push("NOT begins_with(version, :dev_prefix)");
    }

    if !filters.is_empty() {
        if let Some(obj) = query.as_object_mut() {
            // Better join logic:
            let filter_expression = filters
                .iter()
                .map(|f| format!("({})", f))
                .collect::<Vec<_>>()
                .join(" AND ");
            obj.insert("FilterExpression".to_string(), json!(filter_expression));

            if let Some(vals) = obj
                .get_mut("ExpressionAttributeValues")
                .and_then(|v| v.as_object_mut())
            {
                if !include_deprecated {
                    vals.insert(":false".to_string(), json!(false));
                }
                if !include_dev000 {
                    vals.insert(":dev_prefix".to_string(), json!("0.0.0-dev"));
                }
            }
        }
    }

    query
}

fn _get_all_latest_providers_query(pk: &str) -> Value {
    json!({
        "KeyConditionExpression": "PK = :latest",
        "ExpressionAttributeValues": {":latest": pk},
    })
}

pub fn get_all_module_versions_query(
    module: &str,
    track: &str,
    include_deprecated: bool,
    include_dev000: bool,
) -> Value {
    _get_all_module_versions_query(module, track, include_deprecated, include_dev000)
}

pub fn get_all_stack_versions_query(
    stack: &str,
    track: &str,
    include_deprecated: bool,
    include_dev000: bool,
) -> Value {
    _get_all_module_versions_query(stack, track, include_deprecated, include_dev000)
}

fn _get_all_module_versions_query(
    module: &str,
    track: &str,
    include_deprecated: bool,
    include_dev000: bool,
) -> Value {
    log::info!("_get_all_module_versions_query: module={}, track={}, include_deprecated={}, include_dev000={}", module, track, include_deprecated, include_dev000);
    let id: String = format!("MODULE#{}", get_module_identifier(module, track));
    let mut query = json!({
        "KeyConditionExpression": "PK = :module AND begins_with(SK, :sk)",
        "ExpressionAttributeValues": {":module": id, ":sk": "VERSION#"},
        "ScanIndexForward": false,
    });

    let mut filters = Vec::new();

    if !include_deprecated {
        filters.push("attribute_not_exists(deprecated) OR deprecated = :false");
    }

    if !include_dev000 {
        filters.push("NOT begins_with(version, :dev_prefix)");
    }

    if !filters.is_empty() {
        if let Some(obj) = query.as_object_mut() {
            let filter_expression = filters
                .iter()
                .map(|f| format!("({})", f))
                .collect::<Vec<_>>()
                .join(" AND ");
            obj.insert("FilterExpression".to_string(), json!(filter_expression));

            if let Some(vals) = obj
                .get_mut("ExpressionAttributeValues")
                .and_then(|v| v.as_object_mut())
            {
                if !include_deprecated {
                    vals.insert(":false".to_string(), json!(false));
                }
                if !include_dev000 {
                    vals.insert(":dev_prefix".to_string(), json!("0.0.0-dev"));
                }
            }
        }
    }

    query
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

pub fn get_provider_version_query(provider: &str, version: &str) -> Value {
    let id: String = format!("PROVIDER#{}", provider);
    let version_id = format!("VERSION#{}", zero_pad_semver(version, 3).unwrap());
    json!({
        "KeyConditionExpression": "PK = :provider AND SK = :sk",
        "ExpressionAttributeValues": {":provider": id, ":sk": version_id},
        "Limit": 1,
    })
}

pub fn get_stack_version_query(module: &str, track: &str, version: &str) -> Value {
    get_module_version_query(module, track, version)
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

pub fn get_deployment_history_plans_query(
    project_id: &str,
    region: &str,
    environment: Option<&str>,
) -> Value {
    // Use DeletedIndex to query plans by deleted_PK_base and PK prefix
    let deleted_pk_base = format!("0|PLAN#{}::{}", project_id, region);
    let pk_prefix = if let Some(env) = environment {
        format!(
            "PLAN#{}",
            get_deployment_identifier(project_id, region, "", env)
        )
    } else {
        format!("PLAN#{}::{}", project_id, region)
    };

    json!({
        "IndexName": "DeletedIndex",
        "KeyConditionExpression": "deleted_PK_base = :deleted_pk_base AND begins_with(PK, :pk_prefix)",
        "ExpressionAttributeValues": {
            ":deleted_pk_base": deleted_pk_base,
            ":pk_prefix": pk_prefix,
        },
    })
}

pub fn get_deployment_history_deleted_query(
    project_id: &str,
    region: &str,
    environment: Option<&str>,
) -> Value {
    // Use DeletedIndex with exact match on deleted_PK_base and begins_with on PK
    let deleted_pk_base = format!("1|DEPLOYMENT#{}::{}", project_id, region);

    let mut query = json!({
        "IndexName": "DeletedIndex",
        "KeyConditionExpression": "deleted_PK_base = :deleted_pk_base",
        "FilterExpression": "SK = :metadata",
        "ExpressionAttributeValues": {
            ":deleted_pk_base": deleted_pk_base,
            ":metadata": "METADATA",
        },
    });

    // If environment is specified, add it to FilterExpression
    if let Some(env) = environment {
        let pk_prefix = format!(
            "DEPLOYMENT#{}",
            get_deployment_identifier(project_id, region, "", env)
        );
        query["FilterExpression"] = json!("SK = :metadata AND begins_with(PK, :pk_prefix)");
        if let Some(values) = query
            .get_mut("ExpressionAttributeValues")
            .and_then(|v| v.as_object_mut())
        {
            values.insert(":pk_prefix".to_string(), json!(pk_prefix));
        }
    }

    query
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
    event_type: Option<&str>,
) -> Value {
    let mut query = json!({
        "KeyConditionExpression": "PK = :pk",
        "ExpressionAttributeValues": {":pk": format!("EVENT#{}", get_event_identifier(project_id, region, deployment_id, environment))},
        "ScanIndexForward": false
    });

    if let Some(etype) = event_type {
        if etype == "mutate" {
            query["FilterExpression"] = json!("#event = :apply OR #event = :destroy");
            query["ExpressionAttributeNames"] = json!({"#event": "event"});
            if let Some(values) = query
                .get_mut("ExpressionAttributeValues")
                .and_then(|v| v.as_object_mut())
            {
                values.insert(":apply".to_string(), json!("apply"));
                values.insert(":destroy".to_string(), json!("destroy"));
            }
        } else if etype == "plan" {
            query["FilterExpression"] = json!("#event = :plan");
            query["ExpressionAttributeNames"] = json!({"#event": "event"});
            if let Some(values) = query
                .get_mut("ExpressionAttributeValues")
                .and_then(|v| v.as_object_mut())
            {
                values.insert(":plan".to_string(), json!("plan"));
            }
        } else if !etype.is_empty() {
            query["FilterExpression"] = json!("#event = :event");
            query["ExpressionAttributeNames"] = json!({"#event": "event"});

            if let Some(values) = query
                .get_mut("ExpressionAttributeValues")
                .and_then(|v| v.as_object_mut())
            {
                values.insert(":event".to_string(), json!(etype));
            }
        }
    }

    query
}

pub fn get_all_events_between_query(region: &str, start_epoch: u128, end_epoch: u128) -> Value {
    json!({
        "IndexName": "RegionIndex",
        "KeyConditionExpression": "PK_base_region = :pk_base_region AND SK BETWEEN :start_epoch AND :end_epoch",
        "ExpressionAttributeValues": {
            ":pk_base_region": format!("EVENT#{}", region),
            ":start_epoch": start_epoch.to_string(),
            ":end_epoch": end_epoch.to_string(),
        },
        "ScanIndexForward": false,
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
