// Direct database access implementation - bypasses Lambda and calls DynamoDB directly
use anyhow::{anyhow, Result};
use aws_sdk_dynamodb::error::ProvideErrorMetadata;
use aws_sdk_dynamodb::types::AttributeValue;
use serde_json::{json, Value};
use std::collections::HashMap;

use crate::utils::get_table_name;

async fn get_dynamodb_client(region_opt: Option<&str>) -> aws_sdk_dynamodb::Client {
    use aws_sdk_dynamodb::config::{BehaviorVersion, Credentials, Region};

    if std::env::var("TEST_MODE").is_ok() {
        // Local development mode - use local DynamoDB
        let endpoint = std::env::var("DYNAMODB_ENDPOINT")
            .or_else(|_| std::env::var("AWS_ENDPOINT_URL_DYNAMODB"))
            .expect("DYNAMODB_ENDPOINT or AWS_ENDPOINT_URL_DYNAMODB must be set in TEST_MODE");
        eprintln!("Local mode: Using DynamoDB endpoint: {}", endpoint);

        // Use same credentials as internal-api-local for local DynamoDB
        // DynamoDB Local isolates data by access key
        let credentials = Credentials::new(
            std::env::var("AWS_ACCESS_KEY_ID").unwrap_or_else(|_| "minio".to_string()),
            std::env::var("AWS_SECRET_ACCESS_KEY").unwrap_or_else(|_| "minio123".to_string()),
            None,
            None,
            "local",
        );

        let region_name = region_opt.map(|s| s.to_string()).unwrap_or_else(|| {
            std::env::var("AWS_REGION").expect("AWS_REGION environment variable must be set")
        });
        let region = Region::new(region_name);

        let config = aws_sdk_dynamodb::Config::builder()
            .behavior_version(BehaviorVersion::latest())
            .credentials_provider(credentials)
            .region(region)
            .endpoint_url(endpoint)
            .build();

        aws_sdk_dynamodb::Client::from_conf(config)
    } else {
        // Production mode - use real AWS credentials and DynamoDB service
        let mut config_loader = aws_config::from_env();
        if let Some(region) = region_opt {
            config_loader = config_loader.region(Region::new(region.to_string()));
        }
        let config = config_loader.load().await;
        aws_sdk_dynamodb::Client::new(&config)
    }
}

fn json_value_to_attribute_value(value: &Value) -> Result<AttributeValue> {
    match value {
        Value::String(s) => Ok(AttributeValue::S(s.clone())),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Ok(AttributeValue::N(i.to_string()))
            } else if let Some(f) = n.as_f64() {
                Ok(AttributeValue::N(f.to_string()))
            } else {
                Ok(AttributeValue::S(n.to_string()))
            }
        }
        Value::Bool(b) => Ok(AttributeValue::Bool(*b)),
        Value::Null => Ok(AttributeValue::Null(true)),
        Value::Array(arr) => {
            let list: Result<Vec<_>> = arr.iter().map(json_value_to_attribute_value).collect();
            Ok(AttributeValue::L(list?))
        }
        Value::Object(obj) => {
            let mut map = HashMap::new();
            for (k, v) in obj {
                map.insert(k.clone(), json_value_to_attribute_value(v)?);
            }
            Ok(AttributeValue::M(map))
        }
    }
}

fn attribute_value_to_json(attr: &AttributeValue) -> Result<Value> {
    match attr {
        AttributeValue::S(s) => Ok(Value::String(s.clone())),
        AttributeValue::N(n) => {
            if let Ok(i) = n.parse::<i64>() {
                Ok(json!(i))
            } else if let Ok(f) = n.parse::<f64>() {
                Ok(json!(f))
            } else {
                Ok(Value::String(n.clone()))
            }
        }
        AttributeValue::Bool(b) => Ok(Value::Bool(*b)),
        AttributeValue::Null(_) => Ok(Value::Null),
        AttributeValue::L(list) => {
            let arr: Result<Vec<_>> = list.iter().map(attribute_value_to_json).collect();
            Ok(Value::Array(arr?))
        }
        AttributeValue::M(map) => {
            let mut obj = serde_json::Map::new();
            for (k, v) in map {
                obj.insert(k.clone(), attribute_value_to_json(v)?);
            }
            Ok(Value::Object(obj))
        }
        AttributeValue::Ss(ss) => Ok(Value::Array(
            ss.iter().map(|s| Value::String(s.clone())).collect(),
        )),
        AttributeValue::Ns(ns) => Ok(Value::Array(
            ns.iter().map(|n| Value::String(n.clone())).collect(),
        )),
        _ => Ok(Value::Null),
    }
}

fn dynamodb_item_to_json(item: &HashMap<String, AttributeValue>) -> Result<Value> {
    let mut obj = serde_json::Map::new();
    for (k, v) in item {
        obj.insert(k.clone(), attribute_value_to_json(v)?);
    }
    Ok(Value::Object(obj))
}

pub async fn read_db_direct(table: &str, query: &Value, region_opt: Option<&str>) -> Result<Value> {
    let start_time = std::time::Instant::now();
    log::info!(
        "read_db_direct called with table parameter: '{}', region: {:?}",
        table,
        region_opt
    );

    let current_region = std::env::var("AWS_REGION")
        .map_err(|_| anyhow!("AWS_REGION environment variable must be set"))?;
    let mut table_name = get_table_name(table)?;

    // Resolve region to use for client
    let target_region = region_opt.unwrap_or(&current_region);

    if target_region != &current_region {
        // Fix table name if it contains the current region
        let original_name = table_name.clone();
        table_name = table_name.replace(&current_region, target_region);
        if original_name != table_name {
            log::info!(
                "Adjusted table name from '{}' to '{}' for region {}",
                original_name,
                table_name,
                target_region
            );
        }
    }

    log::info!("Resolved table name: '{}'", table_name);
    let client = get_dynamodb_client(Some(target_region)).await;

    log::info!(
        "Direct DB query to table {}: {}",
        table_name,
        serde_json::to_string_pretty(query).unwrap_or_else(|_| "invalid json".to_string())
    );

    let mut query_builder = client.query().table_name(&table_name);

    if let Some(key_condition) = query.get("KeyConditionExpression") {
        if let Some(expr) = key_condition.as_str() {
            query_builder = query_builder.key_condition_expression(expr);
        }
    }

    if let Some(filter_expr) = query.get("FilterExpression") {
        if let Some(expr) = filter_expr.as_str() {
            query_builder = query_builder.filter_expression(expr);
        }
    }

    if let Some(attr_values) = query.get("ExpressionAttributeValues") {
        if let Some(obj) = attr_values.as_object() {
            for (key, value) in obj {
                let attr_value = json_value_to_attribute_value(value)?;
                query_builder = query_builder.expression_attribute_values(key, attr_value);
            }
        }
    }

    if let Some(attr_names) = query.get("ExpressionAttributeNames") {
        if let Some(obj) = attr_names.as_object() {
            for (key, value) in obj {
                if let Some(name) = value.as_str() {
                    query_builder = query_builder.expression_attribute_names(key, name);
                }
            }
        }
    }

    if let Some(index_name) = query.get("IndexName") {
        if let Some(name) = index_name.as_str() {
            query_builder = query_builder.index_name(name);
        }
    }

    if let Some(exclusive_start_key) = query.get("ExclusiveStartKey") {
        if let Some(obj) = exclusive_start_key.as_object() {
            let mut map = HashMap::new();
            for (k, v) in obj {
                map.insert(k.clone(), json_value_to_attribute_value(v)?);
            }
            query_builder = query_builder.set_exclusive_start_key(Some(map));
        }
    }

    if let Some(limit) = query.get("Limit") {
        if let Some(num) = limit.as_i64() {
            query_builder = query_builder.limit(num as i32);
        }
    }

    if let Some(scan_forward) = query.get("ScanIndexForward") {
        if let Some(val) = scan_forward.as_bool() {
            query_builder = query_builder.scan_index_forward(val);
        }
    }

    let result = query_builder.send().await.map_err(|e| {
        log::error!("DynamoDB query failed for table {}: {:?}", table_name, e);
        anyhow!("DynamoDB query failed for table {}: {}", table_name, e)
    })?;

    let items: Vec<Value> = result
        .items()
        .iter()
        .map(|item| dynamodb_item_to_json(item))
        .collect::<Result<Vec<_>>>()?;

    let mut response = json!({
        "Items": items,
        "Count": result.count(),
    });

    if let Some(last_key) = result.last_evaluated_key() {
        if !last_key.is_empty() {
            response["LastEvaluatedKey"] = dynamodb_item_to_json(last_key)?;
            // Also provide base64-encoded next_token for HTTP API compatibility
            if let Ok(json_key) = dynamodb_item_to_json(last_key) {
                if let Ok(json_str) = serde_json::to_string(&json_key) {
                    use base64::Engine;
                    let token = base64::engine::general_purpose::STANDARD.encode(json_str);
                    response["next_token"] = json!(token);
                }
            }
        }
    }

    if let Some(consumed_capacity) = result.consumed_capacity() {
        response["ConsumedCapacity"] = json!({
            "TableName": consumed_capacity.table_name(),
            "CapacityUnits": consumed_capacity.capacity_units(),
        });
    }

    let elapsed = start_time.elapsed();
    log::info!(
        "DB query to table {} completed in {:.2}ms",
        table_name,
        elapsed.as_secs_f64() * 1000.0
    );

    Ok(response)
}

// Additional helper functions for non-DB operations

fn get_env_var(var_name: &str) -> Result<String> {
    std::env::var(var_name).map_err(|_| anyhow!("Environment variable {} not set", var_name))
}

async fn get_ecs_client(region_opt: Option<&str>) -> aws_sdk_ecs::Client {
    use aws_sdk_ecs::config::{Credentials, Region};

    let credentials = Credentials::new(
        std::env::var("AWS_ACCESS_KEY_ID").unwrap_or_else(|_| "dummy".to_string()),
        std::env::var("AWS_SECRET_ACCESS_KEY").unwrap_or_else(|_| "dummy".to_string()),
        None,
        None,
        "local",
    );

    let region_name = region_opt.map(|s| s.to_string()).unwrap_or_else(|| {
        std::env::var("AWS_REGION").expect("AWS_REGION environment variable must be set")
    });
    let region = Region::new(region_name);

    let config = aws_sdk_ecs::Config::builder()
        .behavior_version(aws_sdk_ecs::config::BehaviorVersion::latest())
        .credentials_provider(credentials)
        .region(region)
        .build();

    aws_sdk_ecs::Client::from_conf(config)
}

async fn get_cloudwatch_logs_client(region_opt: Option<&str>) -> aws_sdk_cloudwatchlogs::Client {
    use aws_sdk_cloudwatchlogs::config::{Credentials, Region};

    let credentials = Credentials::new(
        std::env::var("AWS_ACCESS_KEY_ID").unwrap_or_else(|_| "dummy".to_string()),
        std::env::var("AWS_SECRET_ACCESS_KEY").unwrap_or_else(|_| "dummy".to_string()),
        None,
        None,
        "local",
    );

    let region_name = region_opt.map(|s| s.to_string()).unwrap_or_else(|| {
        std::env::var("AWS_REGION").expect("AWS_REGION environment variable must be set")
    });
    let region = Region::new(region_name);

    let config = aws_sdk_cloudwatchlogs::Config::builder()
        .behavior_version(aws_sdk_cloudwatchlogs::config::BehaviorVersion::latest())
        .credentials_provider(credentials)
        .region(region)
        .build();

    aws_sdk_cloudwatchlogs::Client::from_conf(config)
}

pub async fn get_job_status_direct(job_id: &str, region_opt: Option<&str>) -> Result<Value> {
    let cluster = get_env_var("ECS_CLUSTER")?;
    let client = get_ecs_client(region_opt).await;

    let result = client
        .describe_tasks()
        .cluster(&cluster)
        .tasks(job_id)
        .send()
        .await?;

    let task = result
        .tasks()
        .first()
        .ok_or_else(|| anyhow!("Task not found"))?;

    let status = task.last_status().unwrap_or("UNKNOWN");
    let stopped_reason = task.stopped_reason().unwrap_or("");

    Ok(json!({
        "status": status,
        "stopped_reason": stopped_reason
    }))
}

pub async fn read_logs_direct(
    job_id: &str,
    project_id: &str,
    region: &str,
    next_token: Option<&str>,
    limit: Option<i32>,
) -> Result<Value> {
    log::info!(
        "read_logs_direct: job_id={}, project_id={}, region={}, next_token={:?}, limit={:?}",
        job_id,
        project_id,
        region,
        next_token,
        limit
    );

    let log_group = format!("/infraweave/{}/{}", project_id, region);
    let log_stream = job_id;

    let client = get_cloudwatch_logs_client(Some(region)).await;

    let mut request = client
        .get_log_events()
        .log_group_name(&log_group)
        .log_stream_name(log_stream)
        .start_from_head(true);

    if let Some(token) = next_token {
        request = request.next_token(token);
    }

    if let Some(l) = limit {
        request = request.limit(l);
    }

    let result = request.send().await?;

    let events: Vec<Value> = result
        .events()
        .iter()
        .map(|event| {
            json!({
                "timestamp": event.timestamp().unwrap_or(0),
                "message": event.message().unwrap_or(""),
            })
        })
        .collect();

    Ok(json!({
        "events": events,
        "nextForwardToken": result.next_forward_token(),
        "nextBackwardToken": result.next_backward_token(),
    }))
}

pub fn get_environment_variables_direct() -> Result<Value> {
    Ok(json!({
        "DYNAMODB_TF_LOCKS_TABLE_ARN": std::env::var("DYNAMODB_TF_LOCKS_TABLE_ARN").ok(),
        "TF_STATE_S3_BUCKET": std::env::var("TF_STATE_S3_BUCKET").ok(),
        "REGION": std::env::var("REGION").ok(),
    }))
}

pub async fn transact_write_direct(items: &Value, region_opt: Option<&str>) -> Result<Value> {
    log::info!("transact_write_direct called with region: {:?}", region_opt);
    let items_array = items
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("items must be an array"))?;

    let current_region = std::env::var("AWS_REGION")
        .map_err(|_| anyhow!("AWS_REGION environment variable must be set"))?;
    let target_region = region_opt.unwrap_or(&current_region);

    let client = get_dynamodb_client(region_opt).await;
    let mut transact_items = Vec::new();

    for item in items_array {
        if let Some(put_op) = item.get("Put") {
            let table_key = put_op
                .get("TableName")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing TableName in Put operation"))?;
            let mut table_name = get_table_name(table_key)?;

            if target_region != &current_region {
                let original_name = table_name.clone();
                table_name = table_name.replace(&current_region, target_region);
                if original_name != table_name {
                    log::info!(
                        "Adjusted table name from '{}' to '{}' for region {}",
                        original_name,
                        table_name,
                        target_region
                    );
                } else {
                    log::warn!(
                        "Could not adjust table name '{}' for region {} (pattern '{}' not found)",
                        original_name,
                        target_region,
                        current_region
                    );
                }
            }

            let item_data = put_op
                .get("Item")
                .ok_or_else(|| anyhow::anyhow!("Missing Item in Put operation"))?;
            let dynamo_item = json_to_dynamodb_item_helper(item_data)?;

            let put_request = aws_sdk_dynamodb::types::Put::builder()
                .table_name(table_name)
                .set_item(Some(dynamo_item))
                .build()
                .map_err(|e| anyhow::anyhow!("Failed to build Put request: {}", e))?;

            transact_items.push(
                aws_sdk_dynamodb::types::TransactWriteItem::builder()
                    .put(put_request)
                    .build(),
            );
        } else if let Some(delete_op) = item.get("Delete") {
            let table_key = delete_op
                .get("TableName")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing TableName in Delete operation"))?;
            let mut table_name = get_table_name(table_key)?;

            if target_region != &current_region {
                let original_name = table_name.clone();
                table_name = table_name.replace(&current_region, target_region);
                if original_name != table_name {
                    log::info!(
                        "Adjusted table name from '{}' to '{}' for region {}",
                        original_name,
                        table_name,
                        target_region
                    );
                } else {
                    log::warn!(
                        "Could not adjust table name '{}' for region {} (pattern '{}' not found)",
                        original_name,
                        target_region,
                        current_region
                    );
                }
            }

            let key_data = delete_op
                .get("Key")
                .ok_or_else(|| anyhow::anyhow!("Missing Key in Delete operation"))?;
            let key = json_to_dynamodb_item_helper(key_data)?;

            let delete_request = aws_sdk_dynamodb::types::Delete::builder()
                .table_name(table_name)
                .set_key(Some(key))
                .build()
                .map_err(|e| anyhow::anyhow!("Failed to build Delete request: {}", e))?;

            transact_items.push(
                aws_sdk_dynamodb::types::TransactWriteItem::builder()
                    .delete(delete_request)
                    .build(),
            );
        } else {
            return Err(anyhow::anyhow!("Unknown operation type in transact_write"));
        }
    }

    client
        .transact_write_items()
        .set_transact_items(Some(transact_items))
        .send()
        .await
        .map_err(|e| {
            let error_details = e.to_string();
            let service_error = match e.as_service_error() {
                Some(se) => format!(
                    "Service Error: {:?} - {}",
                    se.code(),
                    se.message().unwrap_or("no message")
                ),
                None => "Not a service error".to_string(),
            };
            anyhow!(
                "Direct transact_write failed: {} (Details: {})",
                error_details,
                service_error
            )
        })?;

    Ok(json!({
        "ResponseMetadata": {
            "HTTPStatusCode": 200
        }
    }))
}

pub async fn insert_db_direct(
    table: &str,
    data: &Value,
    region_opt: Option<&str>,
) -> Result<Value> {
    log::info!(
        "insert_db_direct called with table: {}, region: {:?}",
        table,
        region_opt
    );
    let mut table_name = get_table_name(table)?;

    let current_region = std::env::var("AWS_REGION")
        .map_err(|_| anyhow!("AWS_REGION environment variable must be set"))?;
    let target_region = region_opt.unwrap_or(&current_region);

    if target_region != &current_region {
        let original_name = table_name.clone();
        table_name = table_name.replace(&current_region, target_region);
        if original_name != table_name {
            log::info!(
                "Adjusted table name from '{}' to '{}' for region {}",
                original_name,
                table_name,
                target_region
            );
        } else {
            log::warn!(
                "Could not adjust table name '{}' for region {} (pattern '{}' not found)",
                original_name,
                target_region,
                current_region
            );
        }
    }
    log::info!("Resolved table name: '{}'", table_name);

    let client = get_dynamodb_client(region_opt).await;

    let item = json_to_dynamodb_item_helper(data)?;

    client
        .put_item()
        .table_name(table_name)
        .set_item(Some(item))
        .send()
        .await
        .map_err(|e| {
            // Unpack error to get more details
            let error_details = e.to_string();
            let service_error = match e.as_service_error() {
                Some(se) => format!(
                    "Service Error: {:?} - {}",
                    se.code(),
                    se.message().unwrap_or("no message")
                ),
                None => "Not a service error".to_string(),
            };
            anyhow!(
                "Failed to insert item: {} (Details: {})",
                error_details,
                service_error
            )
        })?;

    Ok(json!({"success": true}))
}

fn json_to_dynamodb_item_helper(json: &Value) -> Result<HashMap<String, AttributeValue>> {
    let obj = json
        .as_object()
        .ok_or_else(|| anyhow::anyhow!("Expected JSON object"))?;

    let mut item = HashMap::new();
    for (key, value) in obj {
        item.insert(key.clone(), json_value_to_attribute_value(value)?);
    }

    Ok(item)
}

// ============= S3 Operations =============

async fn get_s3_client(region_opt: Option<&str>) -> aws_sdk_s3::Client {
    let endpoint_opt = std::env::var("AWS_ENDPOINT_URL_S3")
        .or_else(|_| std::env::var("MINIO_ENDPOINT"))
        .ok();

    if let Some(endpoint) = endpoint_opt {
        use aws_sdk_s3::config::{BehaviorVersion, Credentials, Region};

        let credentials = Credentials::new(
            std::env::var("AWS_ACCESS_KEY_ID").unwrap_or_else(|_| "minio".to_string()),
            std::env::var("AWS_SECRET_ACCESS_KEY").unwrap_or_else(|_| "minio123".to_string()),
            None,
            None,
            "local",
        );

        let region_name = region_opt
            .map(|s| s.to_string())
            .or_else(|| std::env::var("AWS_REGION").ok())
            .unwrap_or_else(|| "us-west-2".to_string());

        aws_sdk_s3::Client::from_conf(
            aws_sdk_s3::Config::builder()
                .behavior_version(BehaviorVersion::latest())
                .credentials_provider(credentials)
                .region(Region::new(region_name))
                .force_path_style(true)
                .endpoint_url(endpoint)
                .build(),
        )
    } else {
        let mut loader = aws_config::from_env();
        if let Some(region) = region_opt {
            loader = loader.region(aws_config::Region::new(region.to_string()));
        }
        let config = loader.load().await;

        let mut builder = aws_sdk_s3::config::Builder::from(&config);
        if std::env::var("AWS_S3_FORCE_PATH_STYLE")
            .map(|v| v == "true")
            .unwrap_or(false)
        {
            builder = builder.force_path_style(true);
        }
        aws_sdk_s3::Client::from_conf(builder.build())
    }
}

pub async fn upload_file_base64_direct(
    bucket_name: &str,
    key: &str,
    base64_content: &str,
    region: Option<&str>,
) -> Result<()> {
    use base64::Engine;

    let content = base64::engine::general_purpose::STANDARD
        .decode(base64_content)
        .map_err(|e| anyhow!("Failed to decode base64: {}", e))?;

    log::info!(
        "Uploading {} bytes to {}/{} (region: {:?})",
        content.len(),
        bucket_name,
        key,
        region
    );

    let client = get_s3_client(region).await;
    client
        .put_object()
        .bucket(bucket_name)
        .key(key)
        .body(content.into())
        .send()
        .await?;

    Ok(())
}

/// Upload a file from URL to S3. Returns true if the object already existed.
pub async fn upload_file_url_direct(
    bucket_name: &str,
    key: &str,
    url: &str,
    region: Option<&str>,
) -> Result<bool> {
    let client = get_s3_client(region).await;

    // Check if object already exists
    if client
        .head_object()
        .bucket(bucket_name)
        .key(key)
        .send()
        .await
        .is_ok()
    {
        return Ok(true);
    }

    let resp = reqwest::get(url).await?;
    let bytes = resp.bytes().await?;

    client
        .put_object()
        .bucket(bucket_name)
        .key(key)
        .body(bytes.to_vec().into())
        .send()
        .await?;

    Ok(false)
}

pub async fn generate_presigned_url_direct(
    bucket_name: &str,
    key: &str,
    expires_in_secs: u64,
    region: Option<&str>,
) -> Result<String> {
    let client = get_s3_client(region).await;
    let presigning_config = aws_sdk_s3::presigning::PresigningConfig::expires_in(
        std::time::Duration::from_secs(expires_in_secs),
    )?;
    let presigned_request = client
        .get_object()
        .bucket(bucket_name)
        .key(key)
        .presigned(presigning_config)
        .await?;
    Ok(presigned_request.uri().to_string())
}

pub async fn download_file_as_string_direct(
    bucket_name: &str,
    key: &str,
    region: Option<&str>,
) -> Result<String> {
    let client = get_s3_client(region).await;
    let object = client
        .get_object()
        .bucket(bucket_name)
        .key(key)
        .send()
        .await?;
    let bytes = object.body.collect().await?.into_bytes();
    Ok(String::from_utf8(bytes.to_vec())?)
}

/// Download a file from S3, returning (bytes, content_length, content_type)
pub async fn download_file_as_bytes_direct(
    bucket_name: &str,
    key: &str,
    region: Option<&str>,
) -> Result<(Vec<u8>, Option<i64>, String)> {
    let client = get_s3_client(region).await;
    let object = client
        .get_object()
        .bucket(bucket_name)
        .key(key)
        .send()
        .await?;
    let content_length = object.content_length;
    let content_type = object.content_type.unwrap_or_else(|| {
        if key.ends_with(".zip") {
            "application/zip".to_string()
        } else {
            "application/octet-stream".to_string()
        }
    });
    let bytes = object.body.collect().await?.into_bytes();
    Ok((bytes.to_vec(), content_length, content_type))
}

// ============= Cross-account operations =============

async fn get_aws_config(region: Option<&str>) -> aws_config::SdkConfig {
    let mut loader = aws_config::from_env();
    if let Some(r) = region {
        loader = loader.region(aws_config::Region::new(r.to_string()));
    }
    loader.load().await
}

async fn assume_role_config(
    project_id: &str,
    role_name: &str,
    session_name: &str,
    region: &str,
) -> Result<aws_config::SdkConfig> {
    let config = get_aws_config(Some(region)).await;
    let sts_client = aws_sdk_sts::Client::new(&config);

    let environment = get_env_var("ENVIRONMENT")?;
    let role_arn = format!(
        "arn:aws:iam::{}:role/{}-{}",
        project_id, role_name, environment
    );
    log::info!("Assuming role: {}", role_arn);

    let assumed_role = sts_client
        .assume_role()
        .role_arn(&role_arn)
        .role_session_name(session_name)
        .send()
        .await
        .map_err(|e| {
            log::error!("Failed to assume role {}: {:?}", role_arn, e);
            anyhow!("Failed to assume role {}: {:?}", role_arn, e)
        })?;

    let credentials = assumed_role
        .credentials()
        .ok_or_else(|| anyhow!("No credentials returned from assume role"))?;

    log::info!("Successfully assumed role in account {}", project_id);

    let creds = aws_credential_types::Credentials::new(
        credentials.access_key_id(),
        credentials.secret_access_key(),
        Some(credentials.session_token().to_string()),
        None,
        "AssumedRole",
    );

    Ok(aws_config::SdkConfig::builder()
        .credentials_provider(aws_credential_types::provider::SharedCredentialsProvider::new(creds))
        .region(aws_config::Region::new(region.to_string()))
        .behavior_version(aws_config::BehaviorVersion::latest())
        .build())
}

pub async fn start_runner_cross_account(data: &Value) -> Result<Value> {
    let project_id = data
        .get("project_id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("Missing 'project_id' in payload"))?;

    let environment = get_env_var("ENVIRONMENT")?;

    let region = data
        .get("region")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("Missing 'region' in payload"))?;

    let cpu = data.get("cpu").and_then(|v| v.as_str()).unwrap_or("256");
    let memory = data.get("memory").and_then(|v| v.as_str()).unwrap_or("512");

    log::info!(
        "Starting runner in project {} region {}",
        project_id,
        region
    );

    let assumed_config = assume_role_config(
        project_id,
        "infraweave_api_execute_runner",
        "CentralApiLaunchRunnerSession",
        region,
    )
    .await?;

    let ecs_client = aws_sdk_ecs::Client::new(&assumed_config);
    let ssm_client = aws_sdk_ssm::Client::new(&assumed_config);

    // Fetch configuration from SSM Parameter Store in the workload account
    log::info!("Fetching configuration from SSM Parameter Store");

    let cluster_param = format!(
        "/infraweave/{}/{}/workload_ecs_cluster_name",
        region, environment
    );
    let subnets_param = format!(
        "/infraweave/{}/{}/workload_ecs_subnet_id",
        region, environment
    );
    let sg_param = format!(
        "/infraweave/{}/{}/workload_ecs_security_group",
        region, environment
    );

    let cluster = ssm_client
        .get_parameter()
        .name(&cluster_param)
        .send()
        .await
        .map_err(|e| {
            anyhow!(
                "Failed to get cluster name from SSM parameter {}: {:?}",
                cluster_param,
                e
            )
        })?
        .parameter()
        .and_then(|p| p.value())
        .ok_or_else(|| anyhow!("No cluster name value in SSM parameter"))?
        .to_string();

    let subnets: Vec<String> = ssm_client
        .get_parameter()
        .name(&subnets_param)
        .send()
        .await
        .map_err(|e| {
            anyhow!(
                "Failed to get subnets from SSM parameter {}: {:?}",
                subnets_param,
                e
            )
        })?
        .parameter()
        .and_then(|p| p.value())
        .ok_or_else(|| anyhow!("No subnets value in SSM parameter"))?
        .split(',')
        .map(|s| s.trim().to_string())
        .collect();

    let security_groups: Vec<String> = ssm_client
        .get_parameter()
        .name(&sg_param)
        .send()
        .await
        .map_err(|e| {
            anyhow!(
                "Failed to get security groups from SSM parameter {}: {}",
                sg_param,
                e
            )
        })?
        .parameter()
        .and_then(|p| p.value())
        .ok_or_else(|| anyhow!("No security groups value in SSM parameter"))?
        .split(',')
        .map(|s| s.trim().to_string())
        .collect();

    let task_definition = format!("infraweave-runner-{}", environment);

    log::info!(
        "Retrieved config - cluster: {}, task_definition: {}, subnets: {:?}, security_groups: {:?}",
        cluster,
        task_definition,
        subnets,
        security_groups
    );

    let payload_json = serde_json::to_string(data)?;

    log::info!(
        "Payload to send to runner (first 200 chars): {}",
        if payload_json.len() > 200 {
            &payload_json[..200]
        } else {
            &payload_json
        }
    );

    let payload_env = aws_sdk_ecs::types::KeyValuePair::builder()
        .name("PAYLOAD")
        .value(payload_json)
        .build();

    let mut env_vars = vec![payload_env];

    if let Some(env_obj) = data.get("environment").and_then(|v| v.as_object()) {
        for (key, value) in env_obj {
            env_vars.push(
                aws_sdk_ecs::types::KeyValuePair::builder()
                    .name(key)
                    .value(value.as_str().unwrap_or(""))
                    .build(),
            );
        }
    }

    let network_config = aws_sdk_ecs::types::NetworkConfiguration::builder()
        .awsvpc_configuration(
            aws_sdk_ecs::types::AwsVpcConfiguration::builder()
                .set_subnets(Some(subnets))
                .set_security_groups(Some(security_groups))
                .assign_public_ip(aws_sdk_ecs::types::AssignPublicIp::Enabled)
                .build()?,
        )
        .build();

    let container_override = aws_sdk_ecs::types::ContainerOverride::builder()
        .name("runner")
        .set_environment(Some(env_vars))
        .cpu(cpu.parse::<i32>()?)
        .memory(memory.parse::<i32>()?)
        .build();

    let task_override = aws_sdk_ecs::types::TaskOverride::builder()
        .container_overrides(container_override)
        .cpu(cpu)
        .memory(memory)
        .build();

    let result = ecs_client
        .run_task()
        .cluster(cluster)
        .task_definition(task_definition)
        .launch_type(aws_sdk_ecs::types::LaunchType::Fargate)
        .network_configuration(network_config)
        .overrides(task_override)
        .send()
        .await
        .map_err(|e| anyhow!("Failed to run ECS task: {:?}", e))?;

    let task_arn = result
        .tasks()
        .first()
        .and_then(|t| t.task_arn())
        .ok_or_else(|| anyhow!("No task ARN returned"))?;

    let job_id = task_arn.split('/').last().unwrap_or(task_arn);

    log::info!("Successfully launched ECS task: {}", task_arn);

    Ok(json!({
        "task_arn": task_arn,
        "job_id": job_id
    }))
}

pub async fn get_job_status_cross_account(
    job_id: &str,
    project_id: &str,
    region: &str,
) -> Result<Value> {
    let environment = std::env::var("ECS_ENVIRONMENT").unwrap_or_else(|_| "prod".to_string());

    let assumed_config = assume_role_config(
        project_id,
        "infraweave_api_read_log",
        "infraweave-job-status-check",
        region,
    )
    .await?;

    // Get cluster name from SSM Parameter Store
    let ssm_client = aws_sdk_ssm::Client::new(&assumed_config);
    let cluster_param_name = format!(
        "/infraweave/{}/{}/workload_ecs_cluster_name",
        region, environment
    );

    let cluster = ssm_client
        .get_parameter()
        .name(&cluster_param_name)
        .send()
        .await
        .map_err(|e| {
            anyhow!(
                "Failed to get SSM parameter {}: {:?}",
                cluster_param_name,
                e
            )
        })?
        .parameter()
        .and_then(|p| p.value())
        .ok_or_else(|| anyhow!("SSM parameter {} has no value", cluster_param_name))?
        .to_string();

    let ecs_client = aws_sdk_ecs::Client::new(&assumed_config);

    let result = ecs_client
        .describe_tasks()
        .cluster(&cluster)
        .tasks(job_id)
        .send()
        .await?;

    let task = result
        .tasks()
        .first()
        .ok_or_else(|| anyhow!("Task not found"))?;

    let status = task.last_status().unwrap_or("UNKNOWN");
    let stopped_reason = task.stopped_reason().unwrap_or("");

    let containers = task.containers();
    let exit_code = if let Some(runner) = containers.iter().find(|c| c.name() == Some("runner")) {
        runner.exit_code().unwrap_or(0)
    } else {
        containers
            .iter()
            .filter_map(|c| c.exit_code())
            .find(|&code| code != 0)
            .unwrap_or(0)
    };

    Ok(json!({
        "status": status,
        "stopped_reason": stopped_reason,
        "exit_code": exit_code
    }))
}

pub async fn read_logs_cross_account(
    job_id: &str,
    project_id: &str,
    region: &str,
    next_token: Option<&str>,
    limit: Option<i32>,
) -> Result<Value> {
    log::info!(
        "read_logs_cross_account: job_id={}, project_id={}, region={}, next_token={:?}, limit={:?}",
        job_id,
        project_id,
        region,
        next_token,
        limit
    );

    let environment = get_env_var("ENVIRONMENT")?;
    let central_account_id = get_env_var("CENTRAL_ACCOUNT_ID")?;

    let log_group = format!("/infraweave/{}/{}/runner", region, environment);
    let log_stream_name = format!("ecs/runner/{}", job_id);

    log::info!(
        "read_logs_cross_account: log_group={}, log_stream_name={}",
        log_group,
        log_stream_name
    );

    let client = if central_account_id == project_id {
        log::info!("Using current account credentials (central account)");
        let config = get_aws_config(None).await;
        aws_sdk_cloudwatchlogs::Client::new(&config)
    } else {
        log::info!("Assuming role in target account: {}", project_id);
        let assumed_config = assume_role_config(
            project_id,
            "infraweave_api_read_log",
            "CentralApiAssumeRoleSession",
            region,
        )
        .await?;
        aws_sdk_cloudwatchlogs::Client::new(&assumed_config)
    };

    let mut request = client
        .get_log_events()
        .log_group_name(&log_group)
        .log_stream_name(&log_stream_name);

    if let Some(token) = next_token {
        log::info!("Using next_token for pagination: {}", token);
        request = request.next_token(token);
    } else {
        request = request.start_from_head(true);
    }

    if let Some(max_items) = limit {
        request = request.limit(max_items);
    }

    let logs_result = request.send().await.map_err(|e| {
        log::error!("Failed to get log events: {:?}", e);
        anyhow!("Failed to get log events: {:?}", e)
    })?;

    let events_count = logs_result.events().len();
    let next_forward_token_result = logs_result.next_forward_token();
    log::info!(
        "Retrieved {} events, input_token={:?}, output_token={:?}",
        events_count,
        next_token,
        next_forward_token_result
    );

    let mut log_str = String::new();
    for event in logs_result.events() {
        if let Some(message) = event.message() {
            log_str.push_str(message);
            log_str.push('\n');
        }
    }

    let is_end_of_stream =
        if let (Some(input_token), Some(output_token)) = (next_token, next_forward_token_result) {
            let same_token = input_token == output_token;
            if same_token {
                log::info!("Same token returned - end of available logs");
            }
            same_token
        } else {
            false
        };

    if is_end_of_stream {
        return Ok(json!({"logs": ""}));
    }

    let mut response = json!({"logs": log_str});
    if let Some(token) = next_forward_token_result {
        response["nextForwardToken"] = json!(token);
    }
    if let Some(token) = logs_result.next_backward_token() {
        response["nextBackwardToken"] = json!(token);
    }

    Ok(response)
}

pub async fn publish_notification_direct(message: &str, subject: Option<&str>) -> Result<Value> {
    let topic_arn = get_env_var("NOTIFICATION_TOPIC_ARN")?;
    let config = get_aws_config(None).await;
    let sns_client = aws_sdk_sns::Client::new(&config);

    let mut request = sns_client.publish().topic_arn(topic_arn).message(message);

    if let Some(subj) = subject {
        request = request.subject(subj);
    }

    let result = request.send().await?;

    Ok(json!({
        "message_id": result.message_id().unwrap_or("")
    }))
}
