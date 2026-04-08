use anyhow::{anyhow, Result};
use aws_config::SdkConfig;
use aws_credential_types::provider::ProvideCredentials;
use aws_sigv4::http_request::{sign, SignableBody, SignableRequest, SigningSettings};
use serde_json::Value;
use std::time::SystemTime;

/// Returns (has_credentials, default_region) for the current AWS environment.
///
/// Useful for CLI login flows that need to check credential availability
/// before attempting authenticated requests (e.g. to fall back to local mode).
pub async fn get_aws_auth_context() -> Result<(bool, String)> {
    let config = aws_config::from_env().load().await;

    let has_credentials = if let Some(provider) = config.credentials_provider() {
        provider.provide_credentials().await.is_ok()
    } else {
        false
    };

    let region = config
        .region()
        .map(|r| r.as_ref().to_string())
        .unwrap_or_else(|| "us-west-2".to_string());

    Ok((has_credentials, region))
}

/// Internal: sign and send an HTTP request using SigV4, returning the raw response.
///
/// If `region_override` is provided, it takes precedence over the region in `config`.
async fn send_signed_request(
    method: &str,
    url: &str,
    body: Option<&Value>,
    config: &SdkConfig,
    region_override: Option<&str>,
) -> Result<reqwest::Response> {
    // Extract region (use override if provided)
    let region = if let Some(r) = region_override {
        r.to_string()
    } else {
        config
            .region()
            .ok_or_else(|| {
                anyhow!("No AWS region configured. Set AWS_REGION environment variable.")
            })?
            .as_ref()
            .to_string()
    };

    // Get credentials from the SDK config
    let credentials_provider = config
        .credentials_provider()
        .ok_or_else(|| anyhow!("No credentials provider found"))?;

    // Resolve credentials
    let credentials = credentials_provider
        .provide_credentials()
        .await
        .map_err(|e| anyhow!("Failed to get credentials: {}", e))?;

    // Prepare the request body
    let body_bytes = if let Some(json_body) = body {
        serde_json::to_vec(json_body)?
    } else {
        Vec::new()
    };

    // Convert credentials to Identity for signing
    let identity = aws_smithy_runtime_api::client::identity::Identity::new(credentials, None);

    // Prepare signing settings and params
    let signing_settings = SigningSettings::default();
    let signing_params = aws_sigv4::sign::v4::SigningParams::builder()
        .identity(&identity)
        .region(&region)
        .name("execute-api")
        .time(SystemTime::now())
        .settings(signing_settings)
        .build()
        .map_err(|e| anyhow!("Failed to build signing params: {}", e))?;

    // Create a signable request
    let signable = SignableRequest::new(
        method,
        url,
        std::iter::empty::<(&str, &str)>(),
        SignableBody::Bytes(&body_bytes),
    )
    .map_err(|e| anyhow!("Failed to create signable request: {}", e))?;

    // Sign the request
    let (signing_instructions, _signature) = sign(signable, &signing_params.into())
        .map_err(|e| anyhow!("Failed to sign request: {}", e))?
        .into_parts();

    // Build the HTTP request with reqwest
    let client = reqwest::Client::new();
    let mut reqwest_request = client.request(reqwest::Method::from_bytes(method.as_bytes())?, url);

    for (name, value) in signing_instructions.headers() {
        reqwest_request = reqwest_request.header(name, value);
    }

    reqwest_request = reqwest_request.header("content-type", "application/json");

    if !body_bytes.is_empty() {
        reqwest_request = reqwest_request.body(body_bytes);
    }

    reqwest_request
        .send()
        .await
        .map_err(|e| anyhow!("Failed to send authenticated request: {}", e))
}

/// Makes an authenticated HTTP call to an AWS API Gateway endpoint using SigV4 signing
///
/// # Arguments
/// * `method` - HTTP method (GET, POST, PUT, DELETE)
/// * `url` - Full URL to the API Gateway endpoint
/// * `body` - Optional JSON body for the request
///
/// # Environment Variables
/// * `AWS_REGION` - AWS region (auto-detected by AWS SDK)
///
/// # Returns
/// The JSON response from the API
pub async fn call_authenticated_http(
    method: &str,
    url: &str,
    body: Option<Value>,
) -> Result<Value> {
    let config = aws_config::from_env().load().await;

    call_authenticated_http_with_config(method, url, body, &config).await
}

/// Makes an authenticated HTTP call using a provided AWS SDK config
///
/// # Arguments
/// * `method` - HTTP method (GET, POST, PUT, DELETE)
/// * `url` - Full URL to the API Gateway endpoint
/// * `body` - Optional JSON body for the request
/// * `config` - AWS SDK configuration
///
/// # Returns
/// The JSON response from the API
pub async fn call_authenticated_http_with_config(
    method: &str,
    url: &str,
    body: Option<Value>,
    config: &SdkConfig,
) -> Result<Value> {
    let response = send_signed_request(method, url, body.as_ref(), config, None).await?;

    if !response.status().is_success() {
        let status = response.status();
        let error_text = response
            .text()
            .await
            .unwrap_or_else(|_| "Unknown error".to_string());
        return Err(anyhow!("API returned error {}: {}", status, error_text));
    }

    // Parse the JSON response
    let result: Value = response
        .json()
        .await
        .map_err(|e| anyhow!("Failed to parse response: {}", e))?;

    Ok(result)
}

/// Makes an authenticated HTTP call, returning the raw status code and body text.
///
/// Unlike `call_authenticated_http`, this does NOT check the status or parse JSON,
/// giving the caller full control over response handling. Supports an optional
/// region override for cases where the region is discovered at runtime (e.g. CLI login).
///
/// # Arguments
/// * `method` - HTTP method (GET, POST, PUT, DELETE)
/// * `url` - Full URL to the API Gateway endpoint
/// * `body` - Optional JSON body for the request
/// * `region_override` - Optional region to use instead of the SDK config region
///
/// # Returns
/// A tuple of (HTTP status code, response body text)
pub async fn call_authenticated_http_raw(
    method: &str,
    url: &str,
    body: Option<Value>,
    region_override: Option<&str>,
) -> Result<(u16, String)> {
    let config = aws_config::from_env().load().await;

    let response =
        send_signed_request(method, url, body.as_ref(), &config, region_override).await?;
    let status = response.status().as_u16();
    let body_text = response
        .text()
        .await
        .map_err(|e| anyhow!("Failed to read response body: {}", e))?;

    Ok((status, body_text))
}
