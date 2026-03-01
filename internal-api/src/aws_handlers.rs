#![cfg(feature = "aws")]
use anyhow::{anyhow, Context, Result};
use aws_sdk_dynamodb::error::ProvideErrorMetadata;
use aws_sdk_dynamodb::operation::RequestId;
use axum::{body::Body, http::header, response::Response};
use base64::{engine::general_purpose, Engine as _};
use log::info;
use serde_dynamo::{from_item, to_attribute_value, to_item};
use serde_json::{json, Value};
use std::collections::HashMap;
use tokio_util::io::ReaderStream;
use tracing::{event, instrument, Level};

use crate::common::get_env_var;

use crate::api_common::DatabaseQuery;
use crate::get_param;

use cached::proc_macro::cached;

// Helper functions to reduce boilerplate
async fn get_aws_config(region: Option<&str>) -> aws_config::SdkConfig {
    let mut loader = aws_config::from_env();
    if let Some(r) = region {
        loader = loader.region(aws_config::Region::new(r.to_string()));
    }
    loader.load().await
}

async fn dynamodb_client(region: Option<&str>) -> aws_sdk_dynamodb::Client {
    aws_sdk_dynamodb::Client::new(&get_aws_config(region).await)
}

async fn s3_client(region: Option<&str>) -> aws_sdk_s3::Client {
    #[cfg(feature = "local")]
    {
        // Local mode: configure for MinIO
        use aws_sdk_s3::config::{BehaviorVersion, Credentials, Region};

        let endpoint = std::env::var("AWS_ENDPOINT_URL_S3")
            .or_else(|_| std::env::var("MINIO_ENDPOINT"))
            .unwrap_or_else(|_| "http://localhost:9000".to_string());

        let credentials = Credentials::new(
            std::env::var("AWS_ACCESS_KEY_ID").unwrap_or_else(|_| "minio".to_string()),
            std::env::var("AWS_SECRET_ACCESS_KEY").unwrap_or_else(|_| "minio123".to_string()),
            None,
            None,
            "local",
        );

        let region_str = region
            .map(|r| r.to_string())
            .or_else(|| std::env::var("AWS_REGION").ok())
            .unwrap_or_else(|| "us-west-2".to_string());

        let config = aws_sdk_s3::Config::builder()
            .behavior_version(BehaviorVersion::latest())
            .credentials_provider(credentials)
            .region(Region::new(region_str))
            .force_path_style(true)
            .endpoint_url(endpoint)
            .build();

        aws_sdk_s3::Client::from_conf(config)
    }

    #[cfg(not(feature = "local"))]
    {
        aws_sdk_s3::Client::new(&get_aws_config(region).await)
    }
}

pub fn get_table_name(table_type: &str, region: Option<&str>) -> Result<String> {
    let env_var = match table_type.to_lowercase().as_str() {
        "events" => "DYNAMODB_EVENTS_TABLE_NAME",
        "modules" => "DYNAMODB_MODULES_TABLE_NAME",
        "deployments" => "DYNAMODB_DEPLOYMENTS_TABLE_NAME",
        "policies" => "DYNAMODB_POLICIES_TABLE_NAME",
        "change_records" | "changerecords" => "DYNAMODB_CHANGE_RECORDS_TABLE_NAME",
        "config" => "DYNAMODB_CONFIG_TABLE_NAME",
        "jobs" => "DYNAMODB_JOBS_TABLE_NAME",
        "permissions" => "DYNAMODB_PERMISSIONS_TABLE_NAME",
        _ => return Err(anyhow!("Unknown table type: {}", table_type)),
    };

    let table_name = get_env_var(env_var)?;

    // If a region is specified, we check if we need to replace the region in the table name
    if let Some(target_region) = region {
        let current_region =
            std::env::var("AWS_REGION").unwrap_or_else(|_| "us-west-2".to_string());

        if target_region != current_region {
            // Check if the current region is part of the table name
            if table_name.contains(&current_region) {
                let new_table_name = table_name.replace(&current_region, target_region);
                info!(
                    "Switched table name from '{}' to '{}' for region '{}'",
                    table_name, new_table_name, target_region
                );
                return Ok(new_table_name);
            }
        }
    }

    Ok(table_name)
}

pub fn get_bucket_name(bucket_type: &str) -> Result<String> {
    let env_var = match bucket_type.to_lowercase().as_str() {
        "modules" => "MODULE_S3_BUCKET",
        "policies" => "POLICY_S3_BUCKET",
        "change_records" | "changerecords" => "CHANGE_RECORD_S3_BUCKET",
        "providers" => "PROVIDERS_S3_BUCKET",
        _ => return Err(anyhow!("Unknown bucket type: {}", bucket_type)),
    };
    get_env_var(env_var)
}

pub fn get_bucket_name_for_region(bucket_type: &str, region: &str) -> Result<String> {
    let bucket_name = get_bucket_name(bucket_type)?;

    // If bucket name already contains a region, replace it
    // Typical format: tf-change-records-{account_id}-{region}-{env}
    // We need to replace the region part

    // Get the current region from env to know what to replace
    let current_region = get_env_var("REGION").unwrap_or_else(|_| "us-west-2".to_string());

    // Replace current region with target region in bucket name
    let updated_bucket =
        bucket_name.replace(&format!("-{}-", current_region), &format!("-{}-", region));

    info!(
        "Bucket name for region '{}': {} -> {}",
        region, bucket_name, updated_bucket
    );

    Ok(updated_bucket)
}

// DatabaseQuery implementation for AWS (DynamoDB)
pub struct AwsDatabase;

impl DatabaseQuery for AwsDatabase {
    async fn query_table(
        &self,
        container: &str,
        query: &Value,
        region: Option<&str>,
    ) -> Result<Value> {
        let mut payload = json!({
            "table": container,
            "data": {
                "query": query
            }
        });

        if let Some(r) = region {
            if let Some(obj) = payload.as_object_mut() {
                obj.insert("region".to_string(), json!(r));
            }
        }

        read_db(&payload).await
    }
}

#[instrument(skip(payload), fields(table = tracing::field::Empty, items = tracing::field::Empty))]
pub async fn insert_db(payload: &Value) -> Result<Value> {
    let table = get_param!(payload, "table");
    let data = payload
        .get("data")
        .ok_or_else(|| anyhow!("Missing 'data' parameter"))?;

    let region = payload.get("region").and_then(|v| v.as_str());

    let table_name = get_table_name(table, region)?;

    // Record table name in span
    tracing::Span::current().record("table", &table_name.as_str());

    let client = dynamodb_client(region).await;
    let item = json_to_dynamodb_item(data)?;

    let result = client
        .put_item()
        .table_name(table_name)
        .set_item(Some(item))
        .send()
        .await?;

    event!(Level::INFO, "DynamoDB put_item completed");

    Ok(json!({
        "ResponseMetadata": {
            "HTTPStatusCode": 200,
            "RequestId": result.request_id().unwrap_or("")
        }
    }))
}

#[instrument(skip(payload), fields(operations = tracing::field::Empty))]
pub async fn transact_write(payload: &Value) -> Result<Value> {
    let operations = payload
        .get("items")
        .and_then(|v| v.as_array())
        .ok_or_else(|| anyhow!("Missing 'items' array"))?;

    tracing::Span::current().record("operations", operations.len());

    let region = payload.get("region").and_then(|v| v.as_str());

    let client = dynamodb_client(region).await;

    let mut transact_items = Vec::new();

    for op in operations {
        if let Some(put_op) = op.get("Put") {
            let table_key = put_op
                .get("TableName")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow!("Missing 'TableName' in Put operation"))?;
            let table_name = get_table_name(table_key, region)?;

            let item_data = put_op
                .get("Item")
                .ok_or_else(|| anyhow!("Missing 'Item' in Put operation"))?;
            let item = json_to_dynamodb_item(item_data)?;

            let put_request = aws_sdk_dynamodb::types::Put::builder()
                .table_name(table_name)
                .set_item(Some(item))
                .build()
                .map_err(|e| anyhow!("Failed to build Put request: {}", e))?;

            transact_items.push(
                aws_sdk_dynamodb::types::TransactWriteItem::builder()
                    .put(put_request)
                    .build(),
            );
        } else if let Some(delete_op) = op.get("Delete") {
            let table_key = delete_op
                .get("TableName")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow!("Missing 'TableName' in Delete operation"))?;
            let table_name = get_table_name(table_key, region)?;

            let key_data = delete_op
                .get("Key")
                .ok_or_else(|| anyhow!("Missing 'Key' in Delete operation"))?;
            let key = json_to_dynamodb_item(key_data)?;

            let delete_request = aws_sdk_dynamodb::types::Delete::builder()
                .table_name(table_name)
                .set_key(Some(key))
                .build()
                .map_err(|e| anyhow!("Failed to build Delete request: {}", e))?;

            transact_items.push(
                aws_sdk_dynamodb::types::TransactWriteItem::builder()
                    .delete(delete_request)
                    .build(),
            );
        } else {
            return Err(anyhow!("Unknown operation type in transact_write"));
        }
    }

    let _result = client
        .transact_write_items()
        .set_transact_items(Some(transact_items))
        .send()
        .await?;

    Ok(json!({
        "ResponseMetadata": {
            "HTTPStatusCode": 200
        }
    }))
}

#[instrument(
    skip(payload),
    fields(table, query_type, items_returned, capacity_units)
)]
pub async fn read_db(payload: &Value) -> Result<Value> {
    let start_time = std::time::Instant::now();
    let table = get_param!(payload, "table");
    let query_data = payload
        .get("data")
        .and_then(|v| v.get("query"))
        .ok_or_else(|| anyhow!("Missing 'query' parameter"))?;

    let region = payload.get("region").and_then(|v| v.as_str());

    let table_name = get_table_name(table, region)?;
    let span = tracing::Span::current();
    span.record("table", &table_name.as_str());

    info!("Querying table '{}' in region '{:?}'", table_name, region);

    let client = dynamodb_client(region).await;

    span.record("query_type", "query");
    let mut query_builder = client.query().table_name(table_name);

    let key_condition = query_data
        .get("KeyConditionExpression")
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            anyhow!(
                "Missing KeyConditionExpression in query for table '{}'. Scan operations are intentionally not supported.",
                table
            )
        })?;
    query_builder = query_builder.key_condition_expression(key_condition);

    if let Some(filter_expr) = query_data.get("FilterExpression") {
        if let Some(expr) = filter_expr.as_str() {
            query_builder = query_builder.filter_expression(expr);
        }
    }

    if let Some(attr_values) = query_data.get("ExpressionAttributeValues") {
        if let Some(obj) = attr_values.as_object() {
            for (key, value) in obj {
                let attr_value = to_attribute_value(value)?;
                query_builder = query_builder.expression_attribute_values(key, attr_value);
            }
        }
    }

    if let Some(attr_names) = query_data.get("ExpressionAttributeNames") {
        if let Some(obj) = attr_names.as_object() {
            for (key, value) in obj {
                if let Some(name) = value.as_str() {
                    query_builder = query_builder.expression_attribute_names(key, name);
                }
            }
        }
    }

    if let Some(index_name) = query_data.get("IndexName") {
        if let Some(name) = index_name.as_str() {
            query_builder = query_builder.index_name(name);
        }
    }

    if let Some(exclusive_start_key) = query_data.get("ExclusiveStartKey") {
        if let Some(obj) = exclusive_start_key.as_object() {
            let mut map = HashMap::new();
            for (k, v) in obj {
                map.insert(k.clone(), to_attribute_value(v)?);
            }
            query_builder = query_builder.set_exclusive_start_key(Some(map));
        }
    }

    if let Some(limit) = query_data.get("Limit") {
        if let Some(num) = limit.as_i64() {
            query_builder = query_builder.limit(num as i32);
        }
    }

    if let Some(scan_forward) = query_data.get("ScanIndexForward") {
        if let Some(val) = scan_forward.as_bool() {
            query_builder = query_builder.scan_index_forward(val);
        }
    }

    let result = query_builder.send().await.map_err(|e| {
        log::error!("DynamoDB query failed: {}", e);
        if let Some(service_err) = e.as_service_error() {
            log::error!("Service error details: {:?}", service_err);
            log::error!("Error message: {:?}", service_err.message());
            log::error!("Error code: {:?}", service_err.code());
        }
        anyhow!("DynamoDB query failed: {}", e)
    })?;

    let items: Vec<Value> = result
        .items()
        .iter()
        .map(|item| from_item(item.clone()))
        .collect::<Result<Vec<_>, _>>()?;

    let mut response = json!({
        "Items": items,
        "Count": result.count(),
    });

    if let Some(last_key) = result.last_evaluated_key() {
        if !last_key.is_empty() {
            if let Ok(json_key) = from_item::<_, Value>(last_key.clone()) {
                if let Ok(json_str) = serde_json::to_string(&json_key) {
                    let token = general_purpose::STANDARD.encode(json_str);
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
    event!(
        Level::INFO,
        duration_ms = elapsed.as_millis() as f64,
        "DynamoDB query completed"
    );
    info!(
        "DB query to table '{}' completed in {:.2}ms. Query: {}",
        table,
        elapsed.as_secs_f64() * 1000.0,
        serde_json::to_string(&query_data).unwrap_or_default()
    );

    Ok(response)
}

#[instrument(skip(payload), fields(bucket, key, file_size_bytes))]
pub async fn upload_file_base64(payload: &Value) -> Result<Value> {
    let data = payload
        .get("data")
        .ok_or_else(|| anyhow!("Missing 'data' parameter"))?;

    let region = payload.get("region").and_then(|v| v.as_str());

    let bucket_key = data
        .get("bucket_name")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("Missing 'bucket_name' parameter"))?;
    let key = data
        .get("key")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("Missing 'key' parameter"))?;
    let content_base64 = data
        .get("base64_content")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("Missing 'base64_content' parameter"))?;

    let bucket_name = get_bucket_name(bucket_key)?;

    let span = tracing::Span::current();
    span.record("bucket", &bucket_name.as_str());
    span.record("key", &key);

    let content = general_purpose::STANDARD
        .decode(content_base64)
        .map_err(|e| anyhow!("Failed to decode base64: {}", e))?;

    span.record("file_size_bytes", content.len());

    let client = s3_client(region).await;

    client
        .put_object()
        .bucket(bucket_name)
        .key(key)
        .body(content.into())
        .send()
        .await?;

    event!(Level::INFO, "S3 upload from base64 completed");

    Ok(json!({
        "statusCode": 200,
        "body": "File uploaded successfully"
    }))
}

#[instrument(
    skip(payload),
    fields(bucket, key, source_url, file_size_bytes, already_exists)
)]
pub async fn upload_file_url(payload: &Value) -> Result<Value> {
    let data = payload
        .get("data")
        .ok_or_else(|| anyhow!("Missing 'data' parameter"))?;

    let region = payload.get("region").and_then(|v| v.as_str());

    let bucket_key = data
        .get("bucket_name")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("Missing 'bucket_name' parameter"))?;
    let key = data
        .get("key")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("Missing 'key' parameter"))?;
    let url = data
        .get("url")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("Missing 'url' parameter"))?;

    let bucket_name = get_bucket_name(bucket_key)?;

    let span = tracing::Span::current();
    span.record("bucket", &bucket_name.as_str());
    span.record("key", &key);
    span.record("source_url", &url);

    let client = s3_client(region).await;

    match client
        .head_object()
        .bucket(&bucket_name)
        .key(key)
        .send()
        .await
    {
        Ok(_) => {
            span.record("already_exists", true);
            event!(Level::INFO, "S3 object already exists, skipping upload");
            return Ok(json!({"object_already_exists": true}));
        }
        Err(_) => {
            span.record("already_exists", false);
        }
    }

    let response = reqwest::get(url).await?;
    let bytes = response.bytes().await?;

    span.record("file_size_bytes", bytes.len());

    client
        .put_object()
        .bucket(bucket_name)
        .key(key)
        .body(bytes.to_vec().into())
        .send()
        .await?;

    event!(Level::INFO, "S3 upload from URL completed");

    Ok(json!({"object_already_exists": false}))
}

#[instrument(skip(payload), fields(bucket, key, expires_in))]
pub async fn generate_presigned_url(payload: &Value) -> Result<Value> {
    let data = payload
        .get("data")
        .ok_or_else(|| anyhow!("Missing 'data' parameter"))?;

    let region = payload.get("region").and_then(|v| v.as_str());

    let key = get_param!(data, "key");
    let bucket_key = data
        .get("bucket_name")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("Missing 'bucket_name' parameter"))?;
    let expires_in = data
        .get("expires_in")
        .and_then(|v| v.as_i64())
        .unwrap_or(3600);

    let bucket_name = get_bucket_name(bucket_key)?;

    let span = tracing::Span::current();
    span.record("bucket", &bucket_name.as_str());
    span.record("key", &key);
    span.record("expires_in", expires_in);
    let client = s3_client(region).await;
    let presigning_config = aws_sdk_s3::presigning::PresigningConfig::expires_in(
        std::time::Duration::from_secs(expires_in as u64),
    )?;

    let presigned_request = client
        .get_object()
        .bucket(bucket_name)
        .key(key)
        .presigned(presigning_config)
        .await?;

    event!(Level::INFO, "Presigned URL generated successfully");

    Ok(json!({
        "url": presigned_request.uri()
    }))
}

#[instrument(
    skip(payload),
    fields(project_id, environment, region, task_definition)
)]
pub async fn start_runner(payload: &Value) -> Result<Value> {
    log::info!(
        "start_runner called with payload keys: {:?}",
        payload.as_object().map(|o| o.keys().collect::<Vec<_>>())
    );
    let data = payload
        .get("data")
        .ok_or_else(|| anyhow!("Missing 'data' parameter"))?;

    // Extract project_id and region from the ApiInfraPayload directly
    let project_id = data
        .get("project_id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("Missing 'project_id' in payload"))?;

    let span = tracing::Span::current();
    span.record("project_id", &project_id);

    let environment = get_env_var("ENVIRONMENT")?;
    span.record("environment", &environment.as_str());

    let _central_region = if let Ok(r) = get_env_var("REGION") {
        if r != "us-west-2" || environment != "dev" {
            r
        } else {
            // If we are in local dev, us-west-2, we should use the payload region
            // This is a hack for local testing to support claims in other regions
            data.get("region")
                .and_then(|v| v.as_str())
                .unwrap_or("us-west-2")
                .to_string()
        }
    } else {
        // Fallback to payload region if REGION env var not set
        data.get("region")
            .and_then(|v| v.as_str())
            .unwrap_or("us-west-2")
            .to_string()
    };

    // Override region from payload if it exists, as the claim dictates where resources are
    // But for SSM parameters, we need to know where the central infrastructure is (where SSM params are stored)
    // The SSM parameters for workload accounts are stored in the region where the API is running (central region)
    // Wait, no. SSM parameters describing the workload account resources (VPC, Subnets) are in the workload account.
    // So we should be looking up SSM parameters in the region specified in the claim (payload.region).

    let payload_region = data
        .get("region")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("Missing 'region' in payload"))?;

    log::info!(
        "Starting runner in project {} region {}",
        project_id,
        payload_region
    );

    // We use the payload region for setting up the clients because that's where the workload resources are
    let region = payload_region;
    span.record("region", &region);

    let cpu = data.get("cpu").and_then(|v| v.as_str()).unwrap_or("256");
    let memory = data.get("memory").and_then(|v| v.as_str()).unwrap_or("512");

    // Always assume role into the workload account to launch ECS task
    event!(
        Level::INFO,
        "Assuming role in workload account for ECS task launch"
    );
    log::info!(
        "Assuming role in workload account {} to launch ECS task",
        project_id
    );
    // Explicitly use the region from payload/env for the STS client client logic as well
    // passing None might default to something else if AWS_REGION is set to something else
    let config = get_aws_config(Some(&region)).await;
    let sts_client = aws_sdk_sts::Client::new(&config);

    let role_arn = format!(
        "arn:aws:iam::{}:role/infraweave_api_execute_runner-{}",
        project_id, environment
    );
    log::info!("Assuming role: {}", role_arn);

    let assumed_role = sts_client
        .assume_role()
        .role_arn(&role_arn)
        .role_session_name("CentralApiLaunchRunnerSession")
        .send()
        .await
        .map_err(|e| {
            log::error!("Failed to assume role {}: {:?}", role_arn, e);
            anyhow!("Failed to assume role to launch runner: {:?}", e)
        })?;

    let credentials = assumed_role
        .credentials()
        .ok_or_else(|| anyhow!("No credentials returned from assume role"))?;

    log::info!("Successfully assumed role in workload account");

    // Create new config with assumed role credentials
    use aws_credential_types::Credentials;
    let creds = Credentials::new(
        credentials.access_key_id(),
        credentials.secret_access_key(),
        Some(credentials.session_token().to_string()),
        None,
        "AssumedRole",
    );

    let new_config = aws_config::SdkConfig::builder()
        .credentials_provider(aws_credential_types::provider::SharedCredentialsProvider::new(creds))
        .region(aws_config::Region::new(region.to_string()))
        .behavior_version(aws_config::BehaviorVersion::latest())
        .build();

    let ecs_client = aws_sdk_ecs::Client::new(&new_config);
    let ssm_client = aws_sdk_ssm::Client::new(&new_config);

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

    let cluster_result = ssm_client
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
        })?;

    let subnets_result = ssm_client
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
        })?;

    let sg_result = ssm_client
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
        })?;

    let cluster = cluster_result
        .parameter()
        .and_then(|p| p.value())
        .ok_or_else(|| anyhow!("No cluster name value in SSM parameter"))?
        .to_string();

    let subnets: Vec<String> = subnets_result
        .parameter()
        .and_then(|p| p.value())
        .ok_or_else(|| anyhow!("No subnets value in SSM parameter"))?
        .split(',')
        .map(|s| s.trim().to_string())
        .collect();

    let security_groups: Vec<String> = sg_result
        .parameter()
        .and_then(|p| p.value())
        .ok_or_else(|| anyhow!("No security groups value in SSM parameter"))?
        .split(',')
        .map(|s| s.trim().to_string())
        .collect();

    // Use standard task definition naming
    let task_definition = format!("infraweave-runner-{}", environment);
    span.record("task_definition", &task_definition.as_str());

    log::info!(
        "Retrieved config - cluster: {}, task_definition: {}, subnets: {:?}, security_groups: {:?}",
        cluster,
        task_definition,
        subnets,
        security_groups
    );

    event!(Level::INFO, cluster = %cluster, subnets = subnets.len(), "ECS configuration retrieved from SSM");

    // Pass ApiInfraPayload as PAYLOAD env var (no variables to avoid size limits)
    // Variables are stored in database and retrieved by runner
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

    let mut environment = vec![payload_env];

    // Also include any additional environment variables from the payload
    if let Some(env_vars) = data.get("environment") {
        if let Some(obj) = env_vars.as_object() {
            for (key, value) in obj {
                let env_var = aws_sdk_ecs::types::KeyValuePair::builder()
                    .name(key)
                    .value(value.as_str().unwrap_or(""))
                    .build();
                environment.push(env_var);
            }
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
        .set_environment(Some(environment))
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

    event!(Level::INFO, task_arn = %task_arn, job_id = %job_id, "ECS task launched successfully");
    log::info!("Successfully launched ECS task: {}", task_arn);

    Ok(json!({
        "task_arn": task_arn,
        "job_id": job_id
    }))
}

#[instrument(skip(payload), fields(job_id, project_id, region, task_status))]
pub async fn get_job_status(payload: &Value) -> Result<Value> {
    let data = payload
        .get("data")
        .ok_or_else(|| anyhow!("Missing 'data' parameter"))?;
    let job_id = data
        .get("job_id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("Missing 'job_id' parameter"))?;

    let project_id = data
        .get("project")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("Missing 'project' parameter"))?;

    let region = data
        .get("region")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("Missing 'region' parameter"))?;

    let span = tracing::Span::current();
    span.record("job_id", &job_id);
    span.record("project_id", &project_id);
    span.record("region", &region);

    // Get environment from ECS_ENVIRONMENT env var or default to "prod"
    let environment = std::env::var("ECS_ENVIRONMENT").unwrap_or_else(|_| "prod".to_string());

    // Assume read-only role in the workload account to check task status
    let role_arn = format!(
        "arn:aws:iam::{}:role/infraweave_api_read_log-{}",
        project_id, environment
    );

    let config = get_aws_config(None).await;
    let sts_client = aws_sdk_sts::Client::new(&config);

    let assumed_role = sts_client
        .assume_role()
        .role_arn(&role_arn)
        .role_session_name("infraweave-job-status-check")
        .send()
        .await
        .context(format!("Failed to assume role: {}", role_arn))?;

    let credentials = assumed_role
        .credentials()
        .ok_or_else(|| anyhow!("No credentials returned from AssumeRole"))?;

    // Create new config with assumed role credentials in the specified region
    use aws_credential_types::Credentials;

    let creds = Credentials::new(
        credentials.access_key_id(),
        credentials.secret_access_key(),
        Some(credentials.session_token().to_string()),
        None,
        "assumed-role",
    );

    let assumed_config = aws_config::SdkConfig::builder()
        .credentials_provider(aws_credential_types::provider::SharedCredentialsProvider::new(creds))
        .region(aws_config::Region::new(region.to_string()))
        .behavior_version(aws_config::BehaviorVersion::latest())
        .build();

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
        .context(format!(
            "Failed to get SSM parameter: {}",
            cluster_param_name
        ))?
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
    span.record("task_status", &status);

    let stopped_reason = task.stopped_reason().unwrap_or("");

    // Check for exit code of the essential container
    let containers = task.containers();
    let exit_code = if let Some(runner) = containers.iter().find(|c| c.name() == Some("runner")) {
        runner.exit_code().unwrap_or(0)
    } else {
        // Fallback: check if any container failed
        containers
            .iter()
            .filter_map(|c| c.exit_code())
            .find(|&code| code != 0)
            .unwrap_or(0)
    };

    event!(Level::INFO, status = %status, exit_code = exit_code, "ECS task status retrieved");

    Ok(json!({
        "status": status,
        "stopped_reason": stopped_reason,
        "exit_code": exit_code
    }))
}

pub async fn read_logs(payload: &Value) -> Result<Value> {
    log::info!(
        "read_logs called with payload: {}",
        serde_json::to_string(payload).unwrap_or_else(|_| "invalid json".to_string())
    );

    let data = payload
        .get("data")
        .ok_or_else(|| anyhow!("Missing 'data' parameter"))?;
    let job_id = data
        .get("job_id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("Missing 'job_id' parameter"))?;
    let project_id = data
        .get("project_id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("Missing 'project_id' parameter"))?;
    let region = data
        .get("region")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("Missing 'region' parameter"))?;

    // Optional pagination parameters
    let next_token = data.get("next_token").and_then(|v| v.as_str());
    let limit = data.get("limit").and_then(|v| v.as_i64()).map(|l| l as i32);

    log::info!(
        "read_logs: job_id={}, project_id={}, region={}, next_token={:?}, limit={:?}",
        job_id,
        project_id,
        region,
        next_token,
        limit
    );

    // Get environment from environment variable
    let environment = get_env_var("ENVIRONMENT").map_err(|e| {
        log::error!("Failed to get ENVIRONMENT variable: {}", e);
        e
    })?;

    let central_account_id = get_env_var("CENTRAL_ACCOUNT_ID").map_err(|e| {
        log::error!("Failed to get CENTRAL_ACCOUNT_ID variable: {}", e);
        e
    })?;

    log::info!(
        "read_logs: environment={}, central_account_id={}",
        environment,
        central_account_id
    );

    // Construct log group and stream names
    let log_group = format!("/infraweave/{}/{}/runner", region, environment);
    let log_stream_name = format!("ecs/runner/{}", job_id);

    log::info!(
        "read_logs: log_group={}, log_stream_name={}",
        log_group,
        log_stream_name
    );

    // Check if we need to assume a role in the target project account
    let client = if central_account_id == project_id {
        log::info!("Using current account credentials (central account)");
        let config = get_aws_config(None).await;
        aws_sdk_cloudwatchlogs::Client::new(&config)
    } else {
        log::info!("Assuming role in target account: {}", project_id);
        let config = get_aws_config(None).await;
        let sts_client = aws_sdk_sts::Client::new(&config);

        let role_arn = format!(
            "arn:aws:iam::{}:role/infraweave_api_read_log-{}",
            project_id, environment
        );
        log::info!("Assuming role: {}", role_arn);

        let assumed_role = sts_client
            .assume_role()
            .role_arn(&role_arn)
            .role_session_name("CentralApiAssumeRoleSession")
            .send()
            .await
            .map_err(|e| {
                log::error!("Failed to assume role {}: {:?}", role_arn, e);
                anyhow!("Failed to assume role: {:?}", e)
            })?;

        let credentials = assumed_role
            .credentials()
            .ok_or_else(|| anyhow!("No credentials returned from assume role"))?;

        log::info!("Successfully assumed role");

        // Create new config with assumed role credentials
        use aws_credential_types::Credentials;
        let creds = Credentials::new(
            credentials.access_key_id(),
            credentials.secret_access_key(),
            Some(credentials.session_token().to_string()),
            None,
            "AssumedRole",
        );

        let new_config = aws_config::SdkConfig::builder()
            .credentials_provider(
                aws_credential_types::provider::SharedCredentialsProvider::new(creds),
            )
            .region(aws_config::Region::new(region.to_string()))
            .behavior_version(aws_config::BehaviorVersion::latest())
            .build();

        aws_sdk_cloudwatchlogs::Client::new(&new_config)
    };

    log::info!("Fetching log events directly from stream...");
    let mut request = client
        .get_log_events()
        .log_group_name(&log_group)
        .log_stream_name(&log_stream_name);

    // Add pagination parameters if provided
    // Note: start_from_head should only be used when NOT using next_token
    if let Some(token) = next_token {
        log::info!("Using next_token for pagination: {}", token);
        request = request.next_token(token);
    } else {
        // Only set start_from_head when not using pagination token
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

    // Concatenate all log messages into a single string (matching webserver-openapi format)
    let mut log_str = String::new();
    for event in logs_result.events() {
        if let Some(message) = event.message() {
            log_str.push_str(message);
            log_str.push('\n');
        }
    }

    // Check if this is the end of the log stream
    // When using a token, CloudWatch returns the same token when there are no MORE events
    // But it still returns the same batch of events around that token
    // So we need to check: same token returned with a token provided = end of stream
    let is_end_of_stream =
        if let (Some(input_token), Some(output_token)) = (next_token, next_forward_token_result) {
            let same_token = input_token == output_token;
            if same_token {
                log::info!("Same token returned - this means we're at the end of available logs");
            }
            same_token
        } else {
            false
        };

    // If we're at the end, return empty logs and no token
    if is_end_of_stream {
        log::info!("End of stream detected - returning empty response");
        return Ok(json!({
            "logs": ""
        }));
    }

    let mut response = json!({
        "logs": log_str
    });

    // Include pagination tokens
    if let Some(next_forward_token) = next_forward_token_result {
        response["nextForwardToken"] = json!(next_forward_token);
        log::info!("Next forward token: {}", next_forward_token);
    }
    if let Some(next_backward_token) = logs_result.next_backward_token() {
        response["nextBackwardToken"] = json!(next_backward_token);
    }

    Ok(response)
}

pub async fn publish_notification(payload: &Value) -> Result<Value> {
    let data = payload
        .get("data")
        .ok_or_else(|| anyhow!("Missing 'data' parameter"))?;
    let message = data
        .get("message")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("Missing 'message' parameter"))?;
    let subject = data.get("subject").and_then(|v| v.as_str());

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

pub async fn get_environment_variables(
    _payload: &serde_json::Value,
) -> anyhow::Result<serde_json::Value> {
    Ok(json!({
        "DYNAMODB_TF_LOCKS_TABLE_ARN": std::env::var("DYNAMODB_TF_LOCKS_TABLE_ARN").ok(),
        "TF_STATE_S3_BUCKET": std::env::var("TF_STATE_S3_BUCKET").ok(),
        "REGION": std::env::var("REGION").ok(),
    }))
}

fn json_to_dynamodb_item(
    json: &Value,
) -> Result<HashMap<String, aws_sdk_dynamodb::types::AttributeValue>> {
    to_item(json).map_err(|e| anyhow!("{}", e))
}

// API routes from webserver-openapi - MOVED TO handlers.rs

pub async fn download_file_as_string(bucket_name: &str, key: &str) -> Result<String> {
    download_file_as_string_from_region(bucket_name, key, None).await
}

pub async fn download_file_as_string_from_region(
    bucket_name: &str,
    key: &str,
    region: Option<&str>,
) -> Result<String> {
    let client = s3_client(region).await;
    let object = client
        .get_object()
        .bucket(bucket_name)
        .key(key)
        .send()
        .await?;

    let bytes = object.body.collect().await?.into_bytes();
    let content = String::from_utf8(bytes.to_vec())?;
    Ok(content)
}

pub async fn download_file(bucket_name: &str, key: &str) -> Result<Response> {
    download_file_from_region(bucket_name, key, None).await
}

pub async fn download_file_from_region(
    bucket_name: &str,
    key: &str,
    region: Option<&str>,
) -> Result<Response> {
    let client = s3_client(region).await;
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

    info!(
        "Downloading file from bucket: {}, key: {}. S3 Content Length: {:?}",
        bucket_name, key, content_length
    );

    // aws_sdk_s3::primitives::ByteStream can be converted to AsyncRead
    let stream = ReaderStream::new(object.body.into_async_read());
    let body = Body::from_stream(stream);

    let mut response = Response::new(body);
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        header::HeaderValue::from_str(&content_type)
            .unwrap_or_else(|_| header::HeaderValue::from_static("application/octet-stream")),
    );
    response.headers_mut().insert(
        header::CONTENT_DISPOSITION,
        header::HeaderValue::from_str(&format!("attachment; filename=\"{}\"", key))
            .unwrap_or_else(|_| header::HeaderValue::from_static("attachment")),
    );

    if let Some(len) = content_length {
        if let Ok(val) = header::HeaderValue::from_str(&len.to_string()) {
            response.headers_mut().insert(header::CONTENT_LENGTH, val);
        }
    }

    Ok(response)
}

pub async fn download_provider(payload: &Value) -> Result<Value> {
    use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};

    let s3_key = payload
        .get("s3_key")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("Missing 's3_key' parameter"))?;

    info!("Downloading provider from S3: {}", s3_key);

    let bucket_name = get_bucket_name("modules")?;
    let client = s3_client(None).await;

    let result = client
        .get_object()
        .bucket(&bucket_name)
        .key(s3_key)
        .send()
        .await
        .map_err(|e| anyhow!("Failed to download provider from S3: {}", e))?;

    let bytes = result
        .body
        .collect()
        .await
        .map_err(|e| anyhow!("Failed to read S3 object body: {}", e))?;

    let zip_bytes = bytes.into_bytes();
    let zip_base64 = BASE64.encode(&zip_bytes);

    Ok(json!({
        "zip_base64": zip_base64
    }))
}

pub async fn publish_module(payload: &Value) -> Result<Value> {
    use env_common::logic::upload_module;
    use env_defs::{CloudProvider, ModuleResp};

    let zip_base64 = payload
        .get("zip_base64")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("Missing 'zip_base64' parameter"))?;

    let module_json = payload
        .get("module")
        .ok_or_else(|| anyhow!("Missing 'module' parameter"))?;

    // Deserialize the module metadata
    let module: ModuleResp = serde_json::from_value(module_json.clone())
        .map_err(|e| anyhow!("Failed to deserialize module: {}", e))?;

    info!(
        "Uploading module {} version {} to all regions",
        module.module, module.version
    );

    // Get handler using default (uses AWS SDK config from environment)
    let handler = env_common::interface::GenericCloudHandler::default().await;
    let all_regions = handler.get_all_regions().await?;

    // Upload module to all regions
    for region in all_regions.iter() {
        let region_handler = handler.copy_with_region(region).await;
        upload_module(&region_handler, &module, &zip_base64.to_string())
            .await
            .map_err(|e| anyhow!("Failed to upload module to region {}: {}", region, e))?;
        info!("Module uploaded to region {}", region);
    }

    info!("Module uploaded successfully to all regions");

    Ok(json!({
        "status": "success",
        "message": format!("Module {} version {} uploaded", module.module, module.version)
    }))
}

pub async fn get_publish_job_status(payload: &Value) -> Result<Value> {
    use env_defs::get_publish_job_identifier;

    let job_id = payload
        .get("job_id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("Missing 'job_id' parameter"))?;

    let job_key = get_publish_job_identifier(job_id);

    // Query DynamoDB for job status
    let client = dynamodb_client(None).await;
    let table_name = get_table_name("jobs", None)?;

    let result = client
        .get_item()
        .table_name(&table_name)
        .key("pk", aws_sdk_dynamodb::types::AttributeValue::S(job_key))
        .send()
        .await?;

    let item = result.item().ok_or_else(|| anyhow!("Job not found"))?;

    // Convert DynamoDB item to JSON
    let job_data = from_item(item.clone())?;

    Ok(job_data)
}

#[cached(
    time = 300, // Cache for 5 minutes (300 seconds)
    result = true, // Only cache Ok results
    sync_writes = true, // Prevent stampedes
    key = "String",
    convert = r#"{ user_id.to_string() }"#
)]
pub async fn get_user_allowed_projects(user_id: &str) -> Result<Vec<String>> {
    log::info!(
        "Cache miss for user_id: {}. Fetching permissions from DynamoDB.",
        user_id
    );
    // 1. Get the table name
    let table_name = get_table_name("permissions", None)?;
    let client = dynamodb_client(None).await;

    // 2. Query the permissions table for the user
    // Assumes Schema: PK = "user_id"
    let result = client
        .get_item()
        .table_name(table_name)
        .key(
            "user_id",
            aws_sdk_dynamodb::types::AttributeValue::S(user_id.to_string()),
        )
        .send()
        .await?;

    // 3. Extract the list of allowed projects
    if let Some(item) = result.item {
        if let Some(projects_attr) = item.get("allowed_projects") {
            if let Ok(projects_list) = projects_attr.as_l() {
                let projects: Result<Vec<String>> = projects_list
                    .iter()
                    .map(|p| {
                        p.as_s()
                            .map(|s| s.clone())
                            .map_err(|_| anyhow!("Invalid project ID format"))
                    })
                    .collect();
                return projects;
            }
        }
    }

    // Default: No access if no record found
    Ok(vec![])
}

pub async fn check_project_access(user_id: &str, project_id: &str) -> Result<bool> {
    let allowed = get_user_allowed_projects(user_id).await?;
    Ok(allowed.contains(&project_id.to_string()))
}
