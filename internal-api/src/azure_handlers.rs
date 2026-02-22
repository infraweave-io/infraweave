#![cfg(feature = "azure")]
use anyhow::{anyhow, Result};
use axum::{
    body::Body,
    http::header,
    response::{IntoResponse, Response},
};
use cached::proc_macro::cached;
use log::info;
use serde_json::{json, Value};


use crate::api_common::{self, DatabaseQuery};
use crate::common::get_env_var;
use crate::get_param;
use azure_core::credentials::TokenCredential;
use azure_identity::DeveloperToolsCredential;

// Helper functions to reduce boilerplate
fn get_azure_credential() -> Result<std::sync::Arc<dyn TokenCredential>> {
    Ok(DeveloperToolsCredential::new(None)
        .map_err(|e| anyhow!("Failed to create Azure credentials: {}", e))?)
}

async fn cosmos_container_client(
    table: &str,
) -> Result<azure_data_cosmos::clients::ContainerClient> {
    use azure_data_cosmos::CosmosClient;
    let cosmos_endpoint = get_env_var("COSMOS_DB_ENDPOINT")?;
    let database_name = get_env_var("COSMOS_DB_DATABASE")?;
    let container_name = get_env_var(&format!("COSMOS_CONTAINER_{}", table.to_uppercase()))?;
    let credential = get_azure_credential()?;
    let client = CosmosClient::new(&cosmos_endpoint, credential, None)?;
    Ok(client
        .database_client(&database_name)
        .container_client(&container_name))
}

fn get_id(item: &Value) -> Result<String> {
    // Match Python implementation: create id from PK~SK
    let pk = item
        .get("PK")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("Missing 'PK' field"))?;
    let sk = item
        .get("SK")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("Missing 'SK' field"))?;

    let raw = format!("{}~{}", pk, sk).to_lowercase();
    // Replace non-alphanumeric characters with underscore
    let safe = raw
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect::<String>();

    Ok(safe)
}

// DatabaseQuery implementation for Azure (Cosmos DB)
pub struct AzureDatabase;

impl DatabaseQuery for AzureDatabase {
    async fn query_table(
        &self,
        container: &str,
        query: &Value,
        _region: Option<&str>,
    ) -> Result<Value> {
        let payload = json!({
            "table": container,
            "data": {
                "query": query
            }
        });

        read_db(&payload).await
    }
}

pub async fn insert_db(payload: &Value) -> Result<Value> {
    let table = get_param!(payload, "table");
    let data = payload
        .get("data")
        .ok_or_else(|| anyhow!("Missing 'data' parameter"))?;
    let container_client = cosmos_container_client(table).await?;

    // Add id field generated from PK~SK (matches Python implementation)
    let mut item = data.clone();
    let id = get_id(&item)?;
    item.as_object_mut()
        .ok_or_else(|| anyhow!("Item data is not a JSON object"))?
        .insert("id".to_string(), json!(id));

    // Extract partition key (PK field)
    let pk = get_param!(item, "PK").to_string();

    // Use upsert_item to match Python's upsert behavior (cosmos 0.29+)
    container_client.upsert_item(&pk, &item, None).await?;

    Ok(json!({
        "statusCode": 200,
        "body": "Item inserted successfully"
    }))
}

pub async fn transact_write(payload: &Value) -> Result<Value> {
    let table = get_param!(payload, "table");
    let operations = payload
        .get("data")
        .and_then(|v| v.get("operations"))
        .and_then(|v| v.as_array())
        .ok_or_else(|| anyhow!("Missing 'operations' array"))?;
    let container_client = cosmos_container_client(table).await?;

    for op in operations {
        let op_type = get_param!(op, "type");
        match op_type {
            "Put" => {
                let mut item = op
                    .get("item")
                    .ok_or_else(|| anyhow!("Missing item data"))?
                    .clone();
                if item.get("id").is_none() {
                    let generated_id = get_id(&item)?;
                    item.as_object_mut()
                        .ok_or_else(|| anyhow!("Item data is not a JSON object"))?
                        .insert("id".to_string(), json!(generated_id));
                }
                let pk = get_param!(item, "PK").to_string();
                container_client.upsert_item(&pk, &item, None).await?;
            }
            "Delete" => {
                let key = op.get("key").ok_or_else(|| anyhow!("Missing key data"))?;
                let id = get_param!(key, "id").to_string();
                let pk = get_param!(key, "PK").to_string();
                container_client.delete_item(&pk, &id, None).await?;
            }
            _ => return Err(anyhow!("Unknown operation type: {}", op_type)),
        }
    }

    Ok(json!({
        "statusCode": 200,
        "body": "Transaction completed successfully"
    }))
}

pub async fn read_db(payload: &Value) -> Result<Value> {
    let start_time = std::time::Instant::now();
    let table = get_param!(payload, "table");
    let query_data = payload
        .get("data")
        .and_then(|v| v.get("query"))
        .ok_or_else(|| anyhow!("Missing 'query' parameter"))?;
    let container_client = cosmos_container_client(table).await?;

    let mut query_str = if let Some(query) = query_data.get("query").and_then(|v| v.as_str()) {
        query.to_string()
    } else {
        return Err(anyhow!(
            "Query conversion from DynamoDB format not yet implemented"
        ));
    };

    if let Some(limit) = query_data.get("Limit").and_then(|v| v.as_i64()) {
        if query_str.to_uppercase().starts_with("SELECT *") {
            query_str = query_str.replacen("SELECT *", &format!("SELECT TOP {} *", limit), 1);
        } else if query_str.to_uppercase().starts_with("SELECT") {
            query_str = query_str.replacen("SELECT", &format!("SELECT TOP {}", limit), 1);
        }
    }

    use azure_data_cosmos::{PartitionKey, Query};
    use futures::StreamExt;

    let mut query = Query::from(query_str);

    if let Some(params) = query_data.get("parameters").and_then(|v| v.as_array()) {
        for param in params {
            if let (Some(name), Some(value)) = (
                param.get("name").and_then(|v| v.as_str()),
                param.get("value"),
            ) {
                query = query.with_parameter(name.to_string(), value.clone())?;
            }
        }
    }

    // Use empty partition key for cross-partition queries
    let partition_key = PartitionKey::EMPTY;

    // ItemIterator<FeedPage<Value>> streams individual items, not pages
    let mut items: Vec<Value> = Vec::new();
    let mut item_stream = container_client.query_items::<Value>(query, partition_key, None)?;

    while let Some(item_result) = item_stream.next().await {
        items.push(item_result?);
    }

    let elapsed = start_time.elapsed();
    log::info!(
        "DB query to table '{}' completed in {:.2}ms. Query: {}",
        table,
        elapsed.as_secs_f64() * 1000.0,
        serde_json::to_string(&query_data).unwrap_or_default()
    );

    let count = items.len();
    Ok(json!({
        "Items": items,
        "Count": count,
    }))
}

pub async fn upload_file_base64(payload: &Value) -> Result<Value> {
    use azure_core::Bytes;
    use azure_storage_blob::BlobServiceClient;
    use base64::{engine::general_purpose, Engine as _};

    let data = payload
        .get("data")
        .ok_or_else(|| anyhow!("Missing 'data' parameter"))?;

    let container_name = data
        .get("bucket_name")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("Missing 'bucket_name' parameter"))?;
    let blob_name = data
        .get("key")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("Missing 'key' parameter"))?;
    let content_base64 = data
        .get("base64_content")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("Missing 'base64_content' parameter"))?;

    let storage_account = get_env_var("AZURE_STORAGE_ACCOUNT")?;
    let endpoint = format!("https://{}.blob.core.windows.net", storage_account);
    let credential = get_azure_credential()?;

    let content = general_purpose::STANDARD
        .decode(content_base64)
        .map_err(|e| anyhow!("Failed to decode base64: {}", e))?;

    let blob_service_client = BlobServiceClient::new(&endpoint, Some(credential), None)?;
    let blob_client = blob_service_client
        .blob_container_client(container_name)
        .blob_client(blob_name);

    let content_length = content.len() as u64;
    let bytes = Bytes::from(content);
    blob_client
        .upload(bytes.into(), true, content_length, None)
        .await?;

    Ok(json!({
        "statusCode": 200,
        "body": "File uploaded successfully"
    }))
}

pub async fn upload_file_url(payload: &Value) -> Result<Value> {
    use azure_storage_blob::BlobServiceClient;

    let data = payload
        .get("data")
        .ok_or_else(|| anyhow!("Missing 'data' parameter"))?;

    let container_name = data
        .get("bucket_name")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("Missing 'bucket_name' parameter"))?;
    let blob_name = data
        .get("key")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("Missing 'key' parameter"))?;
    let url = data
        .get("url")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("Missing 'url' parameter"))?;

    let storage_account = get_env_var("AZURE_STORAGE_ACCOUNT")?;
    let endpoint = format!("https://{}.blob.core.windows.net", storage_account);
    let credential = get_azure_credential()?;

    let blob_service_client = BlobServiceClient::new(&endpoint, Some(credential), None)?;
    let blob_client = blob_service_client
        .blob_container_client(container_name)
        .blob_client(blob_name);

    // Check if blob already exists
    match blob_client.get_properties(None).await {
        Ok(_) => {
            return Ok(json!({"object_already_exists": true}));
        }
        Err(e) => {
            // Log at debug level — the blob likely just doesn't exist yet
            log::debug!("Blob existence check returned error (may not exist yet): {}", e);
        }
    }

    // Download from URL and upload to blob
    let response = reqwest::get(url).await?;
    let bytes = response.bytes().await?;

    let content_length = bytes.len() as u64;
    blob_client
        .upload(bytes.into(), true, content_length, None)
        .await?;

    Ok(json!({"object_already_exists": false}))
}

pub async fn generate_presigned_url(payload: &Value) -> Result<Value> {
    let data = payload
        .get("data")
        .ok_or_else(|| anyhow!("Missing 'data' parameter"))?;
    let blob_name = get_param!(data, "key");
    let container_name = data
        .get("bucket_name")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("Missing 'bucket_name' parameter"))?;
    let expires_in = data
        .get("expires_in")
        .and_then(|v| v.as_i64())
        .unwrap_or(3600);

    let storage_account = get_env_var("AZURE_STORAGE_ACCOUNT")?;

    let key = env_azure::sas::get_user_delegation_key(&storage_account, expires_in).await?;
    let url = env_azure::sas::create_user_delegation_sas_url(
        &storage_account,
        container_name,
        blob_name,
        &key,
    )?;

    Ok(json!({
        "url": url
    }))
}

pub async fn start_runner(payload: &Value) -> Result<Value> {
    let data = payload
        .get("data")
        .ok_or_else(|| anyhow!("Missing 'data' parameter"))?;

    let subscription_id = get_env_var("AZURE_SUBSCRIPTION_ID")?;
    let resource_group = get_env_var("ACI_RESOURCE_GROUP")?;
    let container_image = get_env_var("ACI_CONTAINER_IMAGE")?;
    let location = get_env_var("ACI_LOCATION").unwrap_or_else(|_| "eastus".to_string());

    let cpu = data
        .get("cpu")
        .and_then(|v| v.as_str())
        .and_then(|s| s.parse::<f64>().ok())
        .unwrap_or(0.5);
    let memory = data
        .get("memory")
        .and_then(|v| v.as_str())
        .and_then(|s| s.parse::<f64>().ok())
        .unwrap_or(0.5);

    let mut env_vars = Vec::new();
    if let Some(env) = data.get("environment") {
        if let Some(obj) = env.as_object() {
            for (key, value) in obj {
                env_vars.push(json!({
                    "name": key,
                    "value": value.as_str().unwrap_or("")
                }));
            }
        }
    }

    let container_group_name = format!("runner-{}", uuid::Uuid::new_v4());

    let aci_request = json!({
        "location": location,
        "properties": {
            "containers": [{
                "name": "runner",
                "properties": {
                    "image": container_image,
                    "resources": {
                        "requests": {
                            "cpu": cpu,
                            "memoryInGb": memory
                        }
                    },
                    "environmentVariables": env_vars
                }
            }],
            "osType": "Linux",
            "restartPolicy": "Never"
        }
    });

    let credential = get_azure_credential()?;
    let token = credential
        .get_token(&["https://management.azure.com/.default"], None)
        .await?;

    let client = reqwest::Client::new();
    let url = format!(
        "https://management.azure.com/subscriptions/{}/resourceGroups/{}/providers/Microsoft.ContainerInstance/containerGroups/{}?api-version=2021-09-01",
        subscription_id, resource_group, container_group_name
    );

    let response = client
        .put(&url)
        .bearer_auth(token.token.secret())
        .json(&aci_request)
        .send()
        .await?;

    if !response.status().is_success() {
        let error_text = response.text().await?;
        return Err(anyhow!("Failed to create ACI: {}", error_text));
    }

    Ok(json!({
        "job_id": container_group_name
    }))
}

pub async fn get_job_status(payload: &Value) -> Result<Value> {
    let data = payload
        .get("data")
        .ok_or_else(|| anyhow!("Missing 'data' parameter"))?;
    let job_id = get_param!(data, "job_id");

    let subscription_id = get_env_var("AZURE_SUBSCRIPTION_ID")?;
    let resource_group = get_env_var("ACI_RESOURCE_GROUP")?;

    let credential = get_azure_credential()?;
    let token = credential
        .get_token(&["https://management.azure.com/.default"], None)
        .await?;

    let client = reqwest::Client::new();
    let url = format!(
        "https://management.azure.com/subscriptions/{}/resourceGroups/{}/providers/Microsoft.ContainerInstance/containerGroups/{}?api-version=2021-09-01",
        subscription_id, resource_group, job_id
    );

    let response = client
        .get(&url)
        .bearer_auth(token.token.secret())
        .send()
        .await?;

    if !response.status().is_success() {
        return Err(anyhow!("Container instance not found"));
    }

    let result: Value = response.json().await?;
    let state = result
        .pointer("/properties/instanceView/state")
        .and_then(|v| v.as_str())
        .unwrap_or("Unknown");

    Ok(json!({
        "status": state,
        "stopped_reason": ""
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
    let job_id = get_param!(data, "job_id");
    let _project_id = get_param!(data, "project_id");
    let _region = get_param!(data, "region");

    // Optional pagination parameters (Azure Container Instances doesn't support pagination natively)
    let _next_token = data.get("next_token").and_then(|v| v.as_str());
    let tail = data.get("limit").and_then(|v| v.as_i64()).unwrap_or(1000) as usize;

    log::info!("read_logs: job_id={}, tail={}", job_id, tail);

    let subscription_id = get_env_var("AZURE_SUBSCRIPTION_ID")?;
    let resource_group = get_env_var("ACI_RESOURCE_GROUP")?;

    let credential = get_azure_credential()?;
    let token = credential
        .get_token(&["https://management.azure.com/.default"], None)
        .await?;

    let client = reqwest::Client::new();

    // Add tail parameter to limit number of lines returned
    let url = format!(
        "https://management.azure.com/subscriptions/{}/resourceGroups/{}/providers/Microsoft.ContainerInstance/containerGroups/{}/containers/runner/logs?api-version=2021-09-01&tail={}",
        subscription_id, resource_group, job_id, tail
    );

    let response = client
        .get(&url)
        .bearer_auth(token.token.secret())
        .send()
        .await?;

    if !response.status().is_success() {
        let error_text = response
            .text()
            .await
            .unwrap_or_else(|_| "unknown error".to_string());
        log::error!("Failed to retrieve logs: {}", error_text);
        return Err(anyhow!("Failed to retrieve logs: {}", error_text));
    }

    let result: Value = response.json().await?;
    let log_content = result.get("content").and_then(|v| v.as_str()).unwrap_or("");

    // Return logs as plain string to match AWS format
    let response = json!({
        "logs": log_content
    });

    // Note: Azure Container Instances doesn't provide pagination tokens
    // If the user requests pagination, we simulate it by returning empty on subsequent requests

    Ok(response)
}

pub async fn publish_notification(payload: &Value) -> Result<Value> {
    let data = payload
        .get("data")
        .ok_or_else(|| anyhow!("Missing 'data' parameter"))?;
    let message = get_param!(data, "message");
    let _subject = data.get("subject").and_then(|v| v.as_str());

    // TODO: Implement Azure notification (e.g. via Event Grid or Service Bus)
    log::warn!("publish_notification not yet implemented for Azure. Message: {}", message);

    Ok(json!({
        "message_id": uuid::Uuid::new_v4().to_string()
    }))
}

pub async fn get_environment_variables(
    _payload: &serde_json::Value,
) -> anyhow::Result<serde_json::Value> {
    Ok(json!({
        "COSMOS_DB_ENDPOINT": std::env::var("COSMOS_DB_ENDPOINT").ok(),
        "AZURE_STORAGE_ACCOUNT": std::env::var("AZURE_STORAGE_ACCOUNT").ok(),
        "INFRAWEAVE_ENV": std::env::var("INFRAWEAVE_ENV").ok(),
    }))
}

// API routes from webserver-openapi - MOVED TO handlers.rs

pub async fn download_file_as_string(container_name: &str, blob_name: &str) -> Result<String> {
    use azure_storage_blob::BlobServiceClient;
    use futures::StreamExt;

    let storage_account = get_env_var("AZURE_STORAGE_ACCOUNT")?;
    let endpoint = format!("https://{}.blob.core.windows.net", storage_account);
    let credential = get_azure_credential()?;

    let blob_service_client = BlobServiceClient::new(&endpoint, Some(credential), None)?;
    let blob_client = blob_service_client
        .blob_container_client(container_name)
        .blob_client(blob_name);

    let mut stream = blob_client.download(None).await?.into_body();
    let mut data = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        data.extend(chunk);
    }

    Ok(String::from_utf8(data)?)
}

pub async fn download_file(container_name: &str, blob_name: &str) -> Result<Response> {
    use azure_storage_blob::BlobServiceClient;
    use futures::StreamExt;

    let storage_account = get_env_var("AZURE_STORAGE_ACCOUNT")?;
    let endpoint = format!("https://{}.blob.core.windows.net", storage_account);
    let credential = get_azure_credential()?;

    let blob_service_client = BlobServiceClient::new(&endpoint, Some(credential), None)?;
    let blob_client = blob_service_client
        .blob_container_client(container_name)
        .blob_client(blob_name);

    let response = blob_client.download(None).await?;

    let content_length = response
        .headers()
        .iter()
        .find(|(k, _)| k.as_str().eq_ignore_ascii_case("content-length"))
        .and_then(|(_, v)| v.as_str().parse::<u64>().ok());

    let content_type = response
        .headers()
        .iter()
        .find(|(k, _)| k.as_str().eq_ignore_ascii_case("content-type"))
        .map(|(_, v)| v.as_str().to_string())
        .unwrap_or_else(|| "application/octet-stream".to_string());

    let stream = response.into_body();
    let body_stream = stream.map(|res| {
        res.map(|content| content)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))
    });

    let body = Body::from_stream(body_stream);

    let mut response = Response::new(body);
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        header::HeaderValue::from_str(&content_type)
            .unwrap_or_else(|_| header::HeaderValue::from_static("application/octet-stream")),
    );
    response.headers_mut().insert(
        header::CONTENT_DISPOSITION,
        header::HeaderValue::from_str(&format!("attachment; filename=\"{}\"", blob_name))
            .unwrap_or_else(|_| header::HeaderValue::from_static("attachment")),
    );

    if let Some(len) = content_length {
        if let Ok(val) = header::HeaderValue::from_str(&len.to_string()) {
            response.headers_mut().insert(header::CONTENT_LENGTH, val);
        }
    }

    Ok(response)
}

pub async fn publish_module(_payload: &Value) -> Result<Value> {
    Err(anyhow!("publish_module not implemented for Azure yet"))
}

pub async fn get_publish_job_status(_payload: &Value) -> Result<Value> {
    Err(anyhow!(
        "get_publish_job_status not implemented for Azure yet"
    ))
}

#[cached(
    time = 300,
    result = true,
    sync_writes = true,
    key = "String",
    convert = r#"{ user_id.to_string() }"#
)]
pub async fn get_user_allowed_projects(user_id: &str) -> Result<Vec<String>> {
    log::info!(
        "Cache miss for user_id: {}. Fetching permissions from Cosmos DB.",
        user_id
    );
    // 1. Get the container client
    let client = cosmos_container_client("permissions").await?;

    // 2. Query the permissions container for the user
    // Assumes Schema: id = user_id
    // Use a parameterized query to prevent NoSQL injection
    use azure_data_cosmos::Query;
    let query_obj = Query::from("SELECT * FROM c WHERE c.id = @user_id")
        .with_parameter("@user_id", user_id)?;

    let mut stream = client
        .query_documents(query_obj)
        .into_stream::<serde_json::Value>();

    use futures::StreamExt;
    if let Some(Ok(response)) = stream.next().await {
        if let Some(document) = response.results.first() {
            if let Some(projects_attr) = document.get("allowed_projects") {
                if let Ok(projects_list) =
                    serde_json::from_value::<Vec<String>>(projects_attr.clone())
                {
                    return Ok(projects_list);
                }
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
