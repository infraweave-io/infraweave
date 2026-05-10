#![cfg(feature = "aws")]
use anyhow::{anyhow, Result};
use axum::{body::Body, http::header, response::Response};
use serde_json::{json, Value};
use tracing::{error, info, instrument, warn};

use crate::api_common::{DatabaseQuery, JobRunner};
use crate::common::get_env_var;
use crate::get_param;

pub use env_aws_direct::utils::{
    get_bucket_name, get_bucket_name_for_region, get_table_name_for_region,
};

// Backend implementation for AWS (DynamoDB + ECS)
pub struct AwsBackend;

impl DatabaseQuery for AwsBackend {
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

impl JobRunner for AwsBackend {
    async fn liveness_check(&self, record: Value) -> Value {
        ecs_liveness_check(record).await
    }
}

#[instrument(skip(payload), fields(table = tracing::field::Empty, items = tracing::field::Empty))]
pub async fn insert_db(payload: &Value) -> Result<Value> {
    let table = get_param!(payload, "table");
    let data = payload
        .get("data")
        .ok_or_else(|| anyhow!("Missing 'data' parameter"))?;

    let region = payload.get("region").and_then(|v| v.as_str());

    let table_name = get_table_name_for_region(table, region)?;

    tracing::Span::current().record("table", &table_name.as_str());

    env_aws_direct::insert_db_direct(table, data, region).await?;

    info!("DynamoDB put_item completed");

    Ok(json!({
        "ResponseMetadata": {
            "HTTPStatusCode": 200
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
    let items = payload
        .get("items")
        .ok_or_else(|| anyhow!("Missing 'items'"))?;

    env_aws_direct::transact_write_direct(items, region).await
}

#[instrument(
    skip(payload),
    fields(table, query_type, items_returned, capacity_units, query, error)
)]
pub async fn read_db(payload: &Value) -> Result<Value> {
    let start_time = std::time::Instant::now();
    let table = get_param!(payload, "table");
    let query_data = payload
        .get("data")
        .and_then(|v| v.get("query"))
        .ok_or_else(|| anyhow!("Missing 'query' parameter"))?;

    let region = payload.get("region").and_then(|v| v.as_str());

    let span = tracing::Span::current();
    span.record("table", table);
    span.record("query_type", "query");
    span.record("query", query_data.to_string());

    let table_name = get_table_name_for_region(table, region)?;
    span.record("table", &table_name.as_str());

    info!("Querying table '{}' in region '{:?}'", table_name, region);

    let response = env_aws_direct::read_db_direct(table, query_data, region)
        .await
        .inspect_err(|e| {
            span.record("error", e.to_string().as_str());
        })?;

    let elapsed = start_time.elapsed();
    info!(
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

    env_aws_direct::upload_file_base64_direct(&bucket_name, key, content_base64, region).await?;

    info!("S3 upload from base64 completed");

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

    let already_exists =
        env_aws_direct::upload_file_url_direct(&bucket_name, key, url, region).await?;
    span.record("already_exists", already_exists);

    if already_exists {
        info!("S3 object already exists, skipping upload");
        return Ok(json!({"object_already_exists": true}));
    }

    info!("S3 upload from URL completed");
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

    let url =
        env_aws_direct::generate_presigned_url_direct(&bucket_name, key, expires_in as u64, region)
            .await?;

    info!("Presigned URL generated successfully");

    Ok(json!({"url": url}))
}

#[instrument(
    skip(payload),
    fields(project_id, environment, region, task_definition)
)]
pub async fn start_runner(payload: &Value) -> Result<Value> {
    info!(
        "start_runner called with payload keys: {:?}",
        payload.as_object().map(|o| o.keys().collect::<Vec<_>>())
    );
    let data = payload
        .get("data")
        .ok_or_else(|| anyhow!("Missing 'data' parameter"))?;

    let project_id = data
        .get("project_id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("Missing 'project_id' in payload"))?;

    let span = tracing::Span::current();
    span.record("project_id", &project_id);

    let environment = get_env_var("ENVIRONMENT")?;
    span.record("environment", &environment.as_str());

    let region = data
        .get("region")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("Missing 'region' in payload"))?;
    span.record("region", &region);

    let result = env_aws_direct::start_runner_cross_account(data).await?;

    let task_arn = result
        .get("task_arn")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let job_id = result.get("job_id").and_then(|v| v.as_str()).unwrap_or("");

    info!(task_arn = %task_arn, job_id = %job_id, "ECS task launched successfully");

    Ok(result)
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

    let result = env_aws_direct::get_job_status_cross_account(job_id, project_id, region).await?;

    let status = result
        .get("status")
        .and_then(|v| v.as_str())
        .unwrap_or("UNKNOWN");
    span.record("task_status", &status);

    let exit_code = result
        .get("exit_code")
        .and_then(|v| v.as_i64())
        .unwrap_or(0);

    info!(status = %status, exit_code = exit_code, "ECS task status retrieved");

    Ok(result)
}

pub async fn ecs_liveness_check(record: Value) -> Value {
    let mut deployment: env_defs::DeploymentResp = match serde_json::from_value(record.clone()) {
        Ok(d) => d,
        Err(e) => {
            error!(
                "Liveness check skipped: deployment record did not deserialize: {}",
                e
            );
            return record;
        }
    };

    if !deployment.status.is_busy() {
        return record;
    }
    if deployment.job_id.is_empty()
        || deployment.project_id.is_empty()
        || deployment.region.is_empty()
    {
        return record;
    }

    let job_id_short = deployment
        .job_id
        .split('/')
        .last()
        .unwrap_or(&deployment.job_id);
    let js = match env_aws_direct::get_job_status_cross_account(
        job_id_short,
        &deployment.project_id,
        &deployment.region,
    )
    .await
    {
        Ok(v) => v,
        Err(e) => {
            error!(
                "ECS liveness check lookup failed for {}: {}",
                deployment.job_id, e
            );
            return record;
        }
    };

    let task_status = js.get("status").and_then(|v| v.as_str()).unwrap_or("");
    if !matches!(task_status, "STOPPED" | "DEPROVISIONING") {
        return record;
    }

    let reason = js
        .get("stopped_reason")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .unwrap_or("task exited before writing final status");

    warn!(
        job_id = %deployment.job_id,
        task_status = %task_status,
        stopped_reason = %reason,
        "ECS liveness check: flipping busy deployment to failed"
    );

    deployment.status = env_defs::DeploymentStatus::Failed;
    deployment.error_text = format!("ECS task stopped: {}", reason);

    serde_json::to_value(&deployment).unwrap_or(record)
}

pub async fn read_logs(payload: &Value) -> Result<Value> {
    info!(
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

    let next_token = data.get("next_token").and_then(|v| v.as_str());
    let limit = data.get("limit").and_then(|v| v.as_i64()).map(|l| l as i32);

    env_aws_direct::read_logs_cross_account(job_id, project_id, region, next_token, limit).await
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

    env_aws_direct::publish_notification_direct(message, subject).await
}

pub async fn get_environment_variables(
    _payload: &serde_json::Value,
) -> anyhow::Result<serde_json::Value> {
    env_aws_direct::get_environment_variables_direct()
}

pub async fn download_file_as_string(bucket_name: &str, key: &str) -> Result<String> {
    download_file_as_string_from_region(bucket_name, key, None).await
}

pub async fn download_file_as_string_from_region(
    bucket_name: &str,
    key: &str,
    region: Option<&str>,
) -> Result<String> {
    env_aws_direct::download_file_as_string_direct(bucket_name, key, region).await
}

pub async fn download_file(bucket_name: &str, key: &str) -> Result<Response> {
    download_file_from_region(bucket_name, key, None).await
}

pub async fn download_file_from_region(
    bucket_name: &str,
    key: &str,
    region: Option<&str>,
) -> Result<Response> {
    let (bytes, content_length, content_type) =
        env_aws_direct::download_file_as_bytes_direct(bucket_name, key, region).await?;

    info!(
        "Downloading file from bucket: {}, key: {}. Content Length: {:?}",
        bucket_name, key, content_length
    );

    let body = Body::from(bytes);

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

    let (zip_bytes, _, _) =
        env_aws_direct::download_file_as_bytes_direct(&bucket_name, s3_key, None).await?;

    let zip_base64 = BASE64.encode(&zip_bytes);

    Ok(json!({
        "zip_base64": zip_base64
    }))
}

pub async fn publish_provider(payload: &Value) -> Result<Value> {
    use env_common::logic::upload_provider;
    use env_defs::{CloudProvider, ProviderResp};

    let zip_base64 = payload
        .get("zip_base64")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("Missing 'zip_base64' parameter"))?;

    let provider_json = payload
        .get("provider")
        .ok_or_else(|| anyhow!("Missing 'provider' parameter"))?;

    let provider: ProviderResp = serde_json::from_value(provider_json.clone())
        .map_err(|e| anyhow!("Failed to deserialize provider: {}", e))?;

    info!(
        "Uploading provider {} version {} to all regions",
        provider.name, provider.version
    );

    let handler = env_common::interface::GenericCloudHandler::default().await;
    let all_regions = handler.get_all_regions().await?;

    for region in all_regions.iter() {
        let region_handler = handler.copy_with_region(region).await;
        upload_provider(&region_handler, &provider, &zip_base64.to_string())
            .await
            .map_err(|e| anyhow!("Failed to upload provider to region {}: {}", region, e))?;
        info!("Provider uploaded to region {}", region);
    }

    info!("Provider uploaded successfully to all regions");

    Ok(json!({
        "status": "success",
        "message": format!("Provider {} version {} uploaded", provider.name, provider.version)
    }))
}
