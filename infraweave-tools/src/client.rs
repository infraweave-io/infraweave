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
        // Set on the Chat Lambda, whose calls go to the API directly rather than
        // through the frontend CloudFront proxy, which adds this header itself.
        let mut headers = reqwest::header::HeaderMap::new();
        if let Ok(secret) = std::env::var("API_ORIGIN_VERIFY_SECRET") {
            if !secret.is_empty() {
                let mut value = reqwest::header::HeaderValue::from_str(&secret)
                    .context("API_ORIGIN_VERIFY_SECRET is not a valid header value")?;
                value.set_sensitive(true);
                headers.insert("x-origin-verify", value);
            }
        }

        let http = reqwest::Client::builder()
            // The frontend WAF blocks requests without a User-Agent.
            .user_agent(format!(
                "infraweave/{} (infraweave-tools; {}; {})",
                env!("CARGO_PKG_VERSION"),
                std::env::consts::OS,
                std::env::consts::ARCH
            ))
            .default_headers(headers)
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
