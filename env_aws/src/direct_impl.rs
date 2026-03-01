// Direct database access implementation - bypasses Lambda and calls DynamoDB directly
use anyhow::{anyhow, Result};
use aws_sdk_dynamodb::error::ProvideErrorMetadata;
use aws_sdk_dynamodb::types::AttributeValue;
use serde_json::{json, Value};
use std::collections::HashMap;

use crate::utils::get_table_name;

async fn get_dynamodb_client(region_opt: Option<&str>) -> aws_sdk_dynamodb::Client {
    use aws_sdk_dynamodb::config::{BehaviorVersion, Credentials, Region};

    #[cfg(feature = "direct")]
    {
        // Check if we have a custom DynamoDB endpoint configured (for local development)
        let endpoint_opt = std::env::var("DYNAMODB_ENDPOINT")
            .or_else(|_| std::env::var("AWS_ENDPOINT_URL_DYNAMODB"))
            .ok();

        if let Some(endpoint) = endpoint_opt {
            // Local development mode - use local DynamoDB
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

    #[cfg(not(feature = "direct"))]
    {
        // Production mode - use real AWS credentials and DynamoDB
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
    #[cfg(feature = "direct")]
    {
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

    #[cfg(not(feature = "direct"))]
    {
        let mut config_loader = aws_config::from_env();
        if let Some(region) = region_opt {
            config_loader =
                config_loader.region(aws_sdk_ecs::config::Region::new(region.to_string()));
        }
        let config = config_loader.load().await;
        aws_sdk_ecs::Client::new(&config)
    }
}

async fn get_cloudwatch_logs_client(region_opt: Option<&str>) -> aws_sdk_cloudwatchlogs::Client {
    #[cfg(feature = "direct")]
    {
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

    #[cfg(not(feature = "direct"))]
    {
        let mut config_loader = aws_config::from_env();
        if let Some(region) = region_opt {
            config_loader = config_loader.region(aws_sdk_cloudwatchlogs::config::Region::new(
                region.to_string(),
            ));
        }
        let config = config_loader.load().await;
        aws_sdk_cloudwatchlogs::Client::new(&config)
    }
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
