use anyhow::{anyhow, Context, Result};
use serde_json::Value;
use std::time::Duration;

/// Thin authenticated HTTP client for the InfraWeave internal-api.
///
/// Unlike `http_client` in this workspace, the endpoint and bearer token are
/// passed in explicitly - there is no file-based config lookup. This is what
/// lets the chat backend run as a service and pass through the caller's JWT,
/// so existing project-level authorization in internal-api still applies.
#[derive(Clone)]
pub struct ApiClient {
    endpoint: String,
    token: String,
    http: reqwest::Client,
}

impl ApiClient {
    pub fn new(endpoint: impl Into<String>, token: impl Into<String>) -> Result<Self> {
        let http = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(30))
            .build()
            .context("failed to build reqwest client")?;
        Ok(Self {
            endpoint: endpoint.into().trim_end_matches('/').to_string(),
            token: token.into(),
            http,
        })
    }

    pub async fn get_json(&self, path: &str) -> Result<Value> {
        let url = format!("{}{}", self.endpoint, path);
        let resp = self
            .http
            .get(&url)
            .bearer_auth(&self.token)
            .header("Accept", "application/json")
            .send()
            .await
            .with_context(|| format!("GET {url} failed"))?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(anyhow!("GET {url} -> {status}: {body}"));
        }
        resp.json().await.context("invalid JSON response")
    }

    pub async fn get_bytes(&self, path: &str) -> Result<Vec<u8>> {
        let url = format!("{}{}", self.endpoint, path);
        let resp = self
            .http
            .get(&url)
            .bearer_auth(&self.token)
            .header("Accept", "application/octet-stream")
            .send()
            .await
            .with_context(|| format!("GET {url} failed"))?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(anyhow!("GET {url} -> {status}: {body}"));
        }
        Ok(resp
            .bytes()
            .await
            .context("invalid bytes response")?
            .to_vec())
    }

    /// Returns `Ok(None)` for 404 instead of an error - convenient for
    /// "describe this thing if it exists" tools.
    pub async fn get_optional(&self, path: &str) -> Result<Option<Value>> {
        let url = format!("{}{}", self.endpoint, path);
        let resp = self
            .http
            .get(&url)
            .bearer_auth(&self.token)
            .header("Accept", "application/json")
            .send()
            .await
            .with_context(|| format!("GET {url} failed"))?;

        if resp.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(anyhow!("GET {url} -> {status}: {body}"));
        }
        let value: Value = resp.json().await.context("invalid JSON response")?;
        if value.is_null() {
            Ok(None)
        } else {
            Ok(Some(value))
        }
    }
}
