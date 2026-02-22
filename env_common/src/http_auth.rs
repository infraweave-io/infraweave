use anyhow::{anyhow, Result};
use serde_json::Value;

/// Makes an authenticated HTTP call to the internal API using the appropriate cloud provider authentication
///
/// This function automatically detects the cloud provider from the environment and uses
/// the appropriate authentication mechanism:
/// - AWS: SigV4 signing for API Gateway
/// - Azure: Azure AD token authentication
///
/// # Arguments
/// * `method` - HTTP method (GET, POST, PUT, DELETE)
/// * `url` - Full URL to the API endpoint
/// * `body` - Optional JSON body for the request
///
/// # Returns
/// The JSON response from the API
///
/// # Environment Variables
/// - `CLOUD_PROVIDER` - The cloud provider to use (aws, azure, or none)
///
/// # Examples
/// ```no_run
/// use env_common::http_auth::call_authenticated_http;
/// use serde_json::json;
///
/// #[tokio::main]
/// async fn main() -> anyhow::Result<()> {
///     let result = call_authenticated_http(
///         "PUT",
///         "https://api.example.com/api/v1/module/dev/my-module/1.0.0/deprecate",
///         Some(json!({"message": "Deprecated due to security issue"})),
///     ).await?;
///     println!("Result: {:?}", result);
///     Ok(())
/// }
/// ```
pub async fn call_authenticated_http(
    method: &str,
    url: &str,
    body: Option<Value>,
) -> Result<Value> {
    let provider = std::env::var("CLOUD_PROVIDER")
        .unwrap_or_else(|_| "aws".to_string())
        .to_lowercase();

    match provider.as_str() {
        "aws" => env_aws::call_authenticated_http(method, url, body).await,
        "azure" => env_azure::call_authenticated_http(method, url, body).await,
        "none" => {
            // For local development or testing without authentication
            call_unauthenticated_http(method, url, body).await
        }
        _ => Err(anyhow!("Unsupported cloud provider: {}", provider)),
    }
}

/// Makes an unauthenticated HTTP call (for local development)
///
/// # Arguments
/// * `method` - HTTP method (GET, POST, PUT, DELETE)
/// * `url` - Full URL to the API endpoint
/// * `body` - Optional JSON body for the request
///
/// # Returns
/// The JSON response from the API
pub async fn call_unauthenticated_http(
    method: &str,
    url: &str,
    body: Option<Value>,
) -> Result<Value> {
    let client = reqwest::Client::new();
    let mut request = match method.to_uppercase().as_str() {
        "GET" => client.get(url),
        "POST" => client.post(url),
        "PUT" => client.put(url),
        "DELETE" => client.delete(url),
        _ => return Err(anyhow!("Unsupported HTTP method: {}", method)),
    };

    if let Some(json_body) = body {
        request = request.json(&json_body);
    }

    let response = request
        .send()
        .await
        .map_err(|e| anyhow!("Failed to send request: {}", e))?;

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
