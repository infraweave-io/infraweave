use anyhow::{anyhow, Result};
use azure_core::credentials::TokenCredential;
use azure_identity::DeveloperToolsCredential;
use serde_json::Value;

/// Makes an authenticated HTTP call to an Azure endpoint using Azure credentials
///
/// # Arguments
/// * `method` - HTTP method (GET, POST, PUT, DELETE)
/// * `url` - Full URL to the Azure endpoint
/// * `body` - Optional JSON body for the request
///
/// # Returns
/// The JSON response from the API
pub async fn call_authenticated_http(
    method: &str,
    url: &str,
    body: Option<Value>,
) -> Result<Value> {
    // Get Azure credentials (auto-detects: Azure CLI, Azure Developer CLI, or environment variables)
    let credential = DeveloperToolsCredential::new(None)
        .map_err(|e| anyhow!("Failed to create Azure credentials: {}", e))?;

    call_authenticated_http_with_credential(method, url, body, credential).await
}

/// Makes an authenticated HTTP call using a provided Azure credential
///
/// # Arguments
/// * `method` - HTTP method (GET, POST, PUT, DELETE)
/// * `url` - Full URL to the Azure endpoint
/// * `body` - Optional JSON body for the request
/// * `credential` - Azure token credential
///
/// # Returns
/// The JSON response from the API
pub async fn call_authenticated_http_with_credential<T: TokenCredential + 'static>(
    method: &str,
    url: &str,
    body: Option<Value>,
    credential: std::sync::Arc<T>,
) -> Result<Value> {
    // Get an access token for Azure Management API
    // The scope depends on the Azure service being called
    // For Azure API Management or Function Apps, use the management scope
    let scopes = &["https://management.azure.com/.default"];

    let token_response = credential
        .get_token(scopes, None)
        .await
        .map_err(|e| anyhow!("Failed to get Azure access token: {}", e))?;

    let client = reqwest::Client::new();
    let mut request = match method.to_uppercase().as_str() {
        "GET" => client.get(url),
        "POST" => client.post(url),
        "PUT" => client.put(url),
        "DELETE" => client.delete(url),
        _ => return Err(anyhow!("Unsupported HTTP method: {}", method)),
    };

    request = request.header(
        "Authorization",
        format!("Bearer {}", token_response.token.secret()),
    );

    request = request.header("Content-Type", "application/json");

    if let Some(json_body) = body {
        request = request.json(&json_body);
    }

    let response = request
        .send()
        .await
        .map_err(|e| anyhow!("Failed to send authenticated request: {}", e))?;

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
