use anyhow::{anyhow, Context, Result};
use env_defs::{ModuleResp, ProviderResp};
use log::info;
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Debug, Deserialize)]
struct StoredConfig {
    api_endpoint: Option<String>,
}

/// Get the API endpoint from environment variable or config file
fn get_api_endpoint() -> Result<String> {
    // First try environment variable
    if let Ok(endpoint) = std::env::var("INFRAWEAVE_API_ENDPOINT") {
        return Ok(endpoint);
    }

    // Fall back to config file
    let path = env_utils::config_path::get_token_path()?;

    if !path.exists() {
        return Err(anyhow!(
            "No API endpoint configured. Set INFRAWEAVE_API_ENDPOINT environment variable or run 'infraweave login' first."
        ));
    }

    let json = std::fs::read_to_string(&path).context("Failed to read config file")?;
    let config: StoredConfig = serde_json::from_str(&json).context("Failed to parse config")?;

    config
        .api_endpoint
        .ok_or_else(|| anyhow!("No API endpoint in config. Run 'infraweave login' to configure."))
}

/// Get the stored JWT token (try access_token first, fallback to id_token)
fn get_id_token() -> Result<String> {
    let path = env_utils::config_path::get_token_path()?;

    if !path.exists() {
        return Err(anyhow!(
            "Tokens file not found at {}. Please run 'infraweave login' first.",
            path.display()
        ));
    }

    // Check if token file exists
    if !std::path::Path::new(&path).exists() {
        return Err(anyhow!(
            "No authentication token found. Please run 'infraweave login' to authenticate."
        ));
    }

    let json = std::fs::read_to_string(&path)
        .context("Failed to read tokens file. Please run 'infraweave login' to re-authenticate.")?;
    let tokens: Value = serde_json::from_str(&json).context(
        "Failed to parse tokens file. Please run 'infraweave login' to re-authenticate.",
    )?;

    // Use id_token (contains custom attributes like allowed_projects)
    let token = tokens
        .get("id_token")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| anyhow!("No id_token found in tokens file. Please run 'infraweave login' to re-authenticate."))?;

    // Check if token is expired by decoding JWT
    if let Err(e) = check_token_expiry(&token) {
        return Err(anyhow!(
            "Authentication token has expired: {}. Please run 'infraweave login' to re-authenticate.",
            e
        ));
    }

    Ok(token)
}

/// Check if a JWT token is expired
fn check_token_expiry(token: &str) -> Result<()> {
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};

    // JWT format: header.payload.signature
    let parts: Vec<&str> = token.split('.').collect();
    if parts.len() != 3 {
        return Err(anyhow!("Invalid JWT format"));
    }

    // Decode the payload (second part) - JWTs use URL-safe base64 without padding
    let payload_bytes = URL_SAFE_NO_PAD
        .decode(parts[1])
        .context("Failed to decode JWT payload")?;
    let payload_str = String::from_utf8(payload_bytes).context("JWT payload is not valid UTF-8")?;
    let payload: Value =
        serde_json::from_str(&payload_str).context("Failed to parse JWT payload JSON")?;

    // Check expiration time
    if let Some(exp) = payload.get("exp").and_then(|v| v.as_i64()) {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;

        if now >= exp {
            return Err(anyhow!("token expired {} seconds ago", now - exp));
        }
    }

    Ok(())
}

/// Make an authenticated HTTP GET request using JWT token
async fn http_get(path: &str) -> Result<Value> {
    let endpoint = get_api_endpoint()?;
    let token = get_id_token()?;
    let url = format!("{}{}", endpoint.trim_end_matches('/'), path);

    let client = reqwest::Client::new();

    let request = client
        .get(&url)
        .header("Authorization", format!("Bearer {}", token))
        .header("Content-Type", "application/json")
        .header("Accept-Encoding", "gzip, deflate, br, zstd");

    let response = request
        .send()
        .await
        .context(format!("Failed to make request to {}", url))?;

    let status = response.status();
    if !status.is_success() {
        let error_body = response.text().await.unwrap_or_default();

        // Provide helpful message for 401 Unauthorized
        if status == 401 {
            return Err(anyhow!(
                "Authentication failed (401 Unauthorized). Your token may have expired. Please run 'infraweave login' to re-authenticate.\nServer response: {}",
                error_body
            ));
        }

        return Err(anyhow!(
            "API request failed with status {}: {}",
            status,
            error_body
        ));
    }

    response
        .json()
        .await
        .context("Failed to parse JSON response")
}

/// Make an authenticated HTTP POST request using JWT token
pub async fn http_post(path: &str, body: &Value) -> Result<Value> {
    let endpoint = get_api_endpoint()?;
    let token = get_id_token()?;
    let url = format!("{}{}", endpoint.trim_end_matches('/'), path);

    info!("DEBUG: HTTP POST {}", url);
    info!(
        "DEBUG: Payload_json: {}",
        serde_json::to_string_pretty(body).unwrap_or_default()
    );

    let client = reqwest::Client::new();

    let response = client
        .post(&url)
        .header("Authorization", format!("Bearer {}", token))
        .header("Content-Type", "application/json")
        .header("Accept-Encoding", "gzip, deflate, br, zstd")
        .json(body)
        .send()
        .await
        .context(format!("Failed to make request to {}", url))?;

    let status = response.status();
    if !status.is_success() {
        let error_body = response.text().await.unwrap_or_default();

        // Provide helpful message for 401 Unauthorized
        if status == 401 {
            return Err(anyhow!(
                "Authentication failed (401 Unauthorized). Your token may have expired. Please run 'infraweave login' to re-authenticate.\nServer response: {}",
                error_body
            ));
        }

        return Err(anyhow!(
            "API request failed with status {}: {}",
            status,
            error_body
        ));
    }

    response
        .json()
        .await
        .context("Failed to parse JSON response")
}

/// Get all latest modules via HTTP API
pub async fn http_get_all_latest_modules(_track: &str) -> Result<Vec<ModuleResp>> {
    // Note: The HTTP API doesn't filter by track, it returns all latest modules
    // Track filtering happens client-side
    let path = "/api/v1/modules";
    let response = http_get(path).await?;

    // Parse the response - it should be an array of modules
    if let Some(items) = response.as_array() {
        items
            .iter()
            .map(|item| serde_json::from_value(item.clone()).context("Failed to parse module"))
            .collect()
    } else {
        Err(anyhow!("Expected array response from modules endpoint"))
    }
}

/// Get all latest stacks via HTTP API
pub async fn http_get_all_latest_stacks(_track: &str) -> Result<Vec<ModuleResp>> {
    // Note: The HTTP API doesn't filter by track, it returns all latest stacks
    // Track filtering happens client-side
    let path = "/api/v1/stacks";
    let response = http_get(path).await?;

    if let Some(items) = response.as_array() {
        items
            .iter()
            .map(|item| serde_json::from_value(item.clone()).context("Failed to parse stack"))
            .collect()
    } else {
        Err(anyhow!("Expected array response from stacks endpoint"))
    }
}

/// Get all latest providers via HTTP API
pub async fn http_get_all_latest_providers() -> Result<Vec<ProviderResp>> {
    let path = "/api/v1/providers";
    let response = http_get(path).await?;

    if let Some(items) = response.as_array() {
        items
            .iter()
            .map(|item| serde_json::from_value(item.clone()).context("Failed to parse provider"))
            .collect()
    } else {
        Err(anyhow!("Expected array response from providers endpoint"))
    }
}

/// Get a specific module version via HTTP API
pub async fn http_get_module_version(
    track: &str,
    module_name: &str,
    module_version: &str,
) -> Result<ModuleResp> {
    let path = format!(
        "/api/v1/module/{}/{}/{}",
        track, module_name, module_version
    );
    let response = http_get(&path).await?;
    serde_json::from_value(response).context("Failed to parse module version")
}

/// Get all versions for a module via HTTP API
pub async fn http_get_all_versions_for_module(
    track: &str,
    module: &str,
) -> Result<Vec<ModuleResp>> {
    let path = format!("/api/v1/modules/versions/{}/{}", track, module);
    let response = http_get(&path).await?;

    if let Some(items) = response.as_array() {
        items
            .iter()
            .map(|item| {
                serde_json::from_value(item.clone()).context("Failed to parse module version")
            })
            .collect()
    } else {
        Err(anyhow!(
            "Expected array response from module versions endpoint"
        ))
    }
}

/// Get a specific stack version via HTTP API
pub async fn http_get_stack_version(
    track: &str,
    stack_name: &str,
    stack_version: &str,
) -> Result<ModuleResp> {
    let path = format!("/api/v1/stack/{}/{}/{}", track, stack_name, stack_version);
    let response = http_get(&path).await?;
    serde_json::from_value(response).context("Failed to parse stack version")
}

/// Get all versions for a stack via HTTP API
pub async fn http_get_all_versions_for_stack(track: &str, stack: &str) -> Result<Vec<ModuleResp>> {
    let path = format!("/api/v1/stacks/versions/{}/{}", track, stack);
    let response = http_get(&path).await?;

    if let Some(items) = response.as_array() {
        items
            .iter()
            .map(|item| {
                serde_json::from_value(item.clone()).context("Failed to parse stack version")
            })
            .collect()
    } else {
        Err(anyhow!(
            "Expected array response from stack versions endpoint"
        ))
    }
}

/// Get all projects via HTTP API
pub async fn http_get_all_projects() -> Result<Vec<Value>> {
    let path = "/api/v1/projects";
    let response = http_get(path).await?;

    if let Some(items) = response.as_array() {
        Ok(items.clone())
    } else {
        Err(anyhow!("Expected array response from projects endpoint"))
    }
}

/// Get policies for an environment via HTTP API
pub async fn http_get_policies(environment: &str) -> Result<Vec<Value>> {
    let path = format!("/api/v1/policies/{}", environment);
    let response = http_get(&path).await?;

    if let Some(items) = response.as_array() {
        Ok(items.clone())
    } else {
        Err(anyhow!("Expected array response from policies endpoint"))
    }
}

/// Get a specific policy version via HTTP API
pub async fn http_get_policy_version(
    environment: &str,
    policy_name: &str,
    policy_version: &str,
) -> Result<Value> {
    let path = format!(
        "/api/v1/policy/{}/{}/{}",
        environment, policy_name, policy_version
    );
    http_get(&path).await
}

/// Get deployments for a project/region via HTTP API
pub async fn http_get_deployments(project: &str, region: &str) -> Result<Vec<Value>> {
    let path = format!("/api/v1/deployments/{}/{}", project, region);
    let response = http_get(&path).await?;

    if let Some(items) = response.as_array() {
        Ok(items.clone())
    } else {
        Err(anyhow!("Expected array response from deployments endpoint"))
    }
}

/// Get a specific deployment via HTTP API
pub async fn http_describe_deployment(
    project: &str,
    region: &str,
    environment: &str,
    deployment_id: &str,
) -> Result<Value> {
    let path = format!(
        "/api/v1/deployment/{}/{}/{}/{}",
        project, region, environment, deployment_id
    );
    log::info!("http_describe_deployment: path='{}'", path);
    match http_get(&path).await {
        Ok(v) => Ok(v),
        Err(e) => {
            let msg = e.to_string();
            if msg.contains("status 404")
                || (msg.contains("status 500") && msg.contains("Deployment not found"))
            {
                return Ok(Value::Null);
            }
            Err(e)
        }
    }
}

pub async fn http_get_plan_deployment(
    project: &str,
    region: &str,
    environment: &str,
    deployment_id: &str,
    job_id: &str,
) -> Result<Value> {
    let path = format!(
        "/api/v1/plan/{}/{}/{}/{}/{}",
        project, region, environment, deployment_id, job_id
    );
    http_get(&path).await
}

pub async fn http_get_events(
    project: &str,
    region: &str,
    environment: &str,
    deployment_id: &str,
) -> Result<Vec<Value>> {
    let path = format!(
        "/api/v1/events/{}/{}/{}/{}",
        project, region, environment, deployment_id
    );
    let response = http_get(&path).await?;

    if let Some(items) = response.as_array() {
        Ok(items.clone())
    } else {
        Err(anyhow!("Expected array response from events endpoint"))
    }
}

/// Get logs for a job via HTTP API
pub async fn http_get_logs(project: &str, region: &str, job_id: &str) -> Result<String> {
    let path = format!("/api/v1/logs/{}/{}/{}", project, region, job_id);
    let response = http_get(&path).await?;

    // The logs endpoint might return the logs as a string or in a specific format
    if let Some(logs) = response.as_str() {
        Ok(logs.to_string())
    } else if let Some(logs) = response.get("logs").and_then(|v| v.as_str()) {
        Ok(logs.to_string())
    } else {
        // If it's an object, try to serialize it back to a string
        Ok(serde_json::to_string_pretty(&response)?)
    }
}

/// Get change record via HTTP API
pub async fn http_get_change_record(
    project: &str,
    region: &str,
    environment: &str,
    deployment_id: &str,
    job_id: &str,
    change_type: &str,
) -> Result<Value> {
    log::info!(
        "http_get_change_record: project={}, region={}, env={}, dep_id={}, job_id={}, type={}",
        project,
        region,
        environment,
        deployment_id,
        job_id,
        change_type
    );
    let path = format!(
        "/api/v1/change_record/{}/{}/{}/{}/{}/{}",
        project, region, environment, deployment_id, job_id, change_type
    );
    http_get(&path).await
}

pub async fn http_get_job_status(project: &str, region: &str, job_id: &str) -> Result<Value> {
    let path = format!("/api/v1/job_status/{}/{}/{}", project, region, job_id);
    http_get(&path).await
}

/// Check if HTTP mode is enabled (via env var or config file)
pub fn is_http_mode_enabled() -> bool {
    get_api_endpoint().is_ok()
}

/// Publish a module via HTTP API
pub async fn http_publish_module(
    zip_base64: &str,
    track: &str,
    version: &str,
    job_id: &str,
) -> Result<Value> {
    let path = "/api/v1/module/publish";
    let body = serde_json::json!({
        "zip_base64": zip_base64,
        "track": track,
        "version": version,
        "job_id": job_id
    });
    http_post(path, &body).await
}

/// Get publish job status via HTTP API
pub async fn http_get_publish_job_status(job_id: &str) -> Result<Value> {
    let path = format!("/api/v1/module/publish/{}", job_id);
    http_get(&path).await
}

/// Deprecate a module version via HTTP API (using PUT)
pub async fn http_deprecate_module(
    track: &str,
    module: &str,
    version: &str,
    message: Option<String>,
) -> Result<Value> {
    let endpoint = get_api_endpoint()?;
    let token = get_id_token()?;
    let url = format!(
        "{}/api/v1/module/{}/{}/{}/deprecate",
        endpoint.trim_end_matches('/'),
        track,
        module,
        version
    );

    let client = reqwest::Client::new();
    let mut body = serde_json::json!({});
    if let Some(msg) = message {
        body["message"] = serde_json::Value::String(msg);
    }

    let response = client
        .put(&url)
        .header("Authorization", format!("Bearer {}", token))
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await
        .context(format!("Failed to make request to {}", url))?;

    let status = response.status();
    if !status.is_success() {
        let error_body = response.text().await.unwrap_or_default();
        return Err(anyhow!(
            "API request failed with status {}: {}",
            status,
            error_body
        ));
    }

    response
        .json()
        .await
        .context("Failed to parse JSON response")
}
/// Download provider content as base64 via HTTP API
pub async fn http_download_provider(s3_key: &str) -> Result<Vec<u8>> {
    use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};

    let path = "/api/v1/provider/download";
    let body = json!({
        "s3_key": s3_key
    });

    let response = http_post(path, &body).await?;

    let zip_base64 = response
        .get("zip_base64")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("Response missing zip_base64 field"))?;

    BASE64
        .decode(zip_base64)
        .context("Failed to decode base64 provider content")
}
