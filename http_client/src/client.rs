use anyhow::{anyhow, Context, Result};
use env_defs::{ApiInfraPayloadWithVariables, DeploymentResp, ModuleResp, ProviderResp};
use log::info;
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::OnceLock;
use std::time::Duration;

/// Sentinel token value used for unauthenticated local mode.
pub const LOCAL_TOKEN: &str = "local";

/// Returns a shared reqwest::Client with sensible timeouts.
/// The client is created once and reused for all requests,
/// avoiding the cost of a new TLS connection pool per request.
fn shared_client() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(30))
            .build()
            .expect("Failed to build HTTP client")
    })
}

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

    // Skip JWT validation for local/unauthenticated mode
    if token == LOCAL_TOKEN {
        return Ok(token);
    }

    // Check if token is expired by decoding JWT
    if let Err(e) = check_token_expiry(&token) {
        return Err(anyhow!(
            "Authentication token has expired: {}. Please run 'infraweave login' to re-authenticate.",
            e
        ));
    }

    Ok(token)
}

/// Extract user identity (email or sub) from the stored JWT token.
pub fn get_token_identity() -> Result<String> {
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};

    let token = get_id_token()?;
    if token == LOCAL_TOKEN {
        return Ok(LOCAL_TOKEN.into());
    }

    let parts: Vec<&str> = token.split('.').collect();
    if parts.len() != 3 {
        return Err(anyhow!("Invalid JWT format"));
    }

    let payload_bytes = URL_SAFE_NO_PAD
        .decode(parts[1])
        .context("Failed to decode JWT payload")?;
    let payload: Value = serde_json::from_str(&String::from_utf8(payload_bytes)?)?;

    // Try common identity claims in order of preference
    for claim in &["email", "preferred_username", "sub"] {
        if let Some(val) = payload.get(*claim).and_then(|v| v.as_str()) {
            if !val.is_empty() {
                return Ok(val.to_string());
            }
        }
    }

    Err(anyhow!("No identity claim found in token"))
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

    let client = shared_client();

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

/// Check if an error represents a 404 Not Found response from the API.
/// Use this instead of string-matching on error messages.
pub fn is_not_found_error(err: &anyhow::Error) -> bool {
    let msg = err.to_string();
    msg.contains("status 404") || msg.contains("404 Not Found")
}

/// Make an authenticated HTTP POST request using JWT token
pub async fn http_post(path: &str, body: &Value) -> Result<Value> {
    let endpoint = get_api_endpoint()?;
    let token = get_id_token()?;
    let url = format!("{}{}", endpoint.trim_end_matches('/'), path);

    log::debug!("HTTP POST {}", url);

    let client = shared_client();

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
    // Never use HTTP mode in integration tests — they use direct Lambda invocations
    if std::env::var("TEST_MODE").is_ok() {
        return false;
    }
    get_api_endpoint().is_ok()
}

/// Publish a module via HTTP API
pub async fn http_publish_module(
    zip_base64: &str,
    module_json: &Value,
    track: &str,
    version: &str,
    job_id: &str,
) -> Result<Value> {
    let path = "/api/v1/module/publish";
    let body = serde_json::json!({
        "zip_base64": zip_base64,
        "module": module_json,
        "track": track,
        "version": version,
        "job_id": job_id
    });
    http_post(path, &body).await
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

    let client = shared_client();
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

/// Publish a provider via HTTP API
pub async fn http_publish_provider(zip_base64: &str, provider_json: &Value) -> Result<Value> {
    let path = "/api/v1/provider/publish";
    let body = serde_json::json!({
        "zip_base64": zip_base64,
        "provider": provider_json
    });
    http_post(path, &body).await
}

/// Publish a stack via HTTP API
pub async fn http_publish_stack(zip_base64: &str, module_json: &Value) -> Result<Value> {
    let payload = serde_json::json!({
        "zip_base64": zip_base64,
        "module": module_json,
    });
    http_post("/api/v1/stack/publish", &payload).await
}

/// Deprecate a stack version via HTTP API (using PUT)
pub async fn http_deprecate_stack(
    track: &str,
    stack: &str,
    version: &str,
    message: Option<String>,
) -> Result<Value> {
    let endpoint = get_api_endpoint()?;
    let token = get_id_token()?;
    let url = format!(
        "{}/api/v1/stack/{}/{}/{}/deprecate",
        endpoint.trim_end_matches('/'),
        track,
        stack,
        version
    );

    let client = shared_client();
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

/// Get the latest version of a module via HTTP API (returns None if not found)
pub async fn http_get_latest_module_version(
    track: &str,
    module: &str,
) -> Result<Option<ModuleResp>> {
    // Fetch all versions for this module/track and return the highest
    match http_get_all_versions_for_module(track, module).await {
        Ok(mut versions) if !versions.is_empty() => {
            versions.sort_by(|a, b| {
                let va = env_utils::semver_parse(&a.version).ok();
                let vb = env_utils::semver_parse(&b.version).ok();
                va.cmp(&vb)
            });
            Ok(versions.into_iter().last())
        }
        Ok(_) => Ok(None),
        Err(e) => {
            if is_not_found_error(&e) {
                Ok(None)
            } else {
                Err(e)
            }
        }
    }
}

/// Get the latest version of a stack via HTTP API (returns None if not found)
pub async fn http_get_latest_stack_version(track: &str, stack: &str) -> Result<Option<ModuleResp>> {
    match http_get_all_versions_for_stack(track, stack).await {
        Ok(mut versions) if !versions.is_empty() => {
            versions.sort_by(|a, b| {
                let va = env_utils::semver_parse(&a.version).ok();
                let vb = env_utils::semver_parse(&b.version).ok();
                va.cmp(&vb)
            });
            Ok(versions.into_iter().last())
        }
        Ok(_) => Ok(None),
        Err(e) => {
            if is_not_found_error(&e) {
                Ok(None)
            } else {
                Err(e)
            }
        }
    }
}

/// Get the latest version of a provider via HTTP API (returns None if not found)
pub async fn http_get_latest_provider_version(provider_name: &str) -> Result<Option<ProviderResp>> {
    let all = http_get_all_latest_providers().await?;
    Ok(all.into_iter().find(|p| p.name == provider_name))
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

/// Submit a claim job via the HTTP API
///
/// Posts the claim payload to the server, which inserts the deployment record
/// and launches the runner task. Returns the job ID.
pub async fn http_submit_claim_job(
    payload_with_variables: &ApiInfraPayloadWithVariables,
) -> Result<String> {
    let payload = &payload_with_variables.payload;

    if payload.project_id == "http-mode-no-project" {
        return Err(anyhow!(
            "Project ID is required for deployment operations in HTTP mode.\n\
            Usage: infraweave <cmd> --project <project-id>"
        ));
    }

    let response = http_post(
        "/api/v1/claim/run",
        &serde_json::to_value(payload_with_variables)?,
    )
    .await
    .map_err(|e| anyhow!("Failed to submit claim via HTTP: {}", e))?;

    let job_id = response["job_id"]
        .as_str()
        .ok_or_else(|| anyhow!("No job_id in response"))?
        .to_string();

    info!("Claim submitted via HTTP, job_id: {}", job_id);

    Ok(job_id)
}

/// HTTP-mode: Check if a deployment plan job is still in progress
///
/// Uses the HTTP API to check ECS task status and fetch the plan deployment record.
pub async fn http_is_deployment_plan_in_progress(
    project_id: &str,
    region: &str,
    deployment_id: &str,
    environment: &str,
    job_id: &str,
) -> (bool, String, Option<DeploymentResp>) {
    let job_id_short = job_id.split('/').last().unwrap_or(job_id);

    if let Ok(job_status) = http_get_job_status(project_id, region, job_id_short).await {
        let status = job_status
            .get("status")
            .and_then(|v| v.as_str())
            .unwrap_or("UNKNOWN");
        let stopped_reason = job_status
            .get("stopped_reason")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let exit_code = job_status
            .get("exit_code")
            .and_then(|v| v.as_i64())
            .unwrap_or(0);

        info!(
            "ECS task status: {}, stopped_reason: {}, exit_code: {}",
            status, stopped_reason, exit_code
        );

        let fetch_plan_deployment = async {
            http_get_plan_deployment(project_id, region, environment, deployment_id, job_id_short)
                .await
                .ok()
                .and_then(|v| serde_json::from_value::<DeploymentResp>(v).ok())
        };

        if status == "STOPPED" || status == "DEPROVISIONING" {
            if exit_code != 0 {
                log::error!(
                    "ECS task failed with exit code {}: {}",
                    exit_code,
                    stopped_reason
                );
            } else if !stopped_reason.is_empty()
                && stopped_reason != "Essential container in task exited"
            {
                log::warn!("ECS task stopped with reason: {}", stopped_reason);
            }

            let deployment = fetch_plan_deployment.await;
            if let Some(ref d) = deployment {
                info!(
                    "Found deployment plan record for job {}: status={}",
                    job_id_short, d.status
                );
            } else {
                log::warn!(
                    "Job {} finished with exit code {} but no deployment plan record found yet",
                    job_id_short,
                    exit_code
                );
            }
            return (false, job_id_short.to_string(), deployment);
        } else if matches!(
            status,
            "RUNNING" | "PENDING" | "PROVISIONING" | "ACTIVATING"
        ) {
            let deployment = fetch_plan_deployment.await;
            return (true, job_id_short.to_string(), deployment);
        }
    }

    // Fallback: couldn't determine status from job check
    (true, job_id_short.to_string(), None)
}

/// HTTP-mode: Check the progress of a standard deployment (apply/destroy)
///
/// Uses the HTTP API to check ECS task status and fetch the deployment record.
pub async fn http_check_deployment_progress(
    project_id: &str,
    region: &str,
    deployment_id: &str,
    environment: &str,
    job_id: &str,
) -> (bool, String, Option<DeploymentResp>) {
    let job_id_short = job_id.split('/').last().unwrap_or(job_id);

    if let Ok(job_status) = http_get_job_status(project_id, region, job_id_short).await {
        let status = job_status
            .get("status")
            .and_then(|v| v.as_str())
            .unwrap_or("UNKNOWN");
        let stopped_reason = job_status
            .get("stopped_reason")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let exit_code = job_status
            .get("exit_code")
            .and_then(|v| v.as_i64())
            .unwrap_or(0);

        let fetch_deployment = async {
            http_describe_deployment(project_id, region, environment, deployment_id)
                .await
                .ok()
                .and_then(|v| {
                    if v.is_null() {
                        None
                    } else {
                        serde_json::from_value::<DeploymentResp>(v).ok()
                    }
                })
        };

        if status == "STOPPED" || status == "DEPROVISIONING" {
            if exit_code != 0 {
                log::error!(
                    "ECS task failed with exit code {}: {}",
                    exit_code,
                    stopped_reason
                );
            } else if !stopped_reason.is_empty()
                && stopped_reason != "Essential container in task exited"
            {
                log::warn!("ECS task stopped with reason: {}", stopped_reason);
            }

            let deployment = fetch_deployment.await;
            return (false, job_id_short.to_string(), deployment);
        } else if matches!(
            status,
            "RUNNING" | "PENDING" | "PROVISIONING" | "ACTIVATING"
        ) {
            let deployment = fetch_deployment.await;
            return (true, job_id_short.to_string(), deployment);
        }
    }

    // Fallback: couldn't determine status from job check
    (true, job_id_short.to_string(), None)
}
