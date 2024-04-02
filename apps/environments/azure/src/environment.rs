use serde::{Deserialize, Serialize};
use log::{info};

pub async fn run_function(path: &str, payload: serde_json::Value) -> Result<serde_json::Value, anyhow::Error> {
    let client = reqwest::Client::new();

    let function_url = format!("https://example....azurewebsites.net/api/{}", path);
    let function_key = "Vdr...3A==";

    info!("Making request to {} with payload: {}", path, payload);

    let response = client
        .post(function_url)
        .header("x-functions-key", function_key)
        .json(&payload)
        .send()
        .await?
        .text()
        .await?;

    info!("Response payload: {}", response);

    let json_response = serde_json::Value::String(response);

    Ok(json_response)
}