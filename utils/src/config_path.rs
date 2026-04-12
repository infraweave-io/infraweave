use anyhow::{anyhow, Result};
use serde::Deserialize;
use std::path::PathBuf;

#[derive(Debug, Deserialize)]
struct StoredConfig {
    api_endpoint: Option<String>,
}

/// Check if HTTP mode is enabled (cloud-agnostic).
/// Returns true if an API endpoint is configured via INFRAWEAVE_API_ENDPOINT
/// environment variable or the config file (from `infraweave login`).
/// Returns false if TEST_MODE is set (used by the local scaffold to force direct DB access).
pub fn is_http_mode_enabled() -> bool {
    // TEST_MODE explicitly disables HTTP mode (used by the local scaffold during seeding)
    if std::env::var("TEST_MODE").is_ok() {
        return false;
    }
    if std::env::var("INFRAWEAVE_API_ENDPOINT").is_ok() {
        return true;
    }
    if let Ok(path) = get_token_path() {
        if path.exists() {
            if let Ok(json) = std::fs::read_to_string(&path) {
                if let Ok(config) = serde_json::from_str::<StoredConfig>(&json) {
                    return config.api_endpoint.is_some();
                }
            }
        }
    }
    false
}

/// Get the path to the infraweave config directory
/// Uses ~/.infraweave/ on all platforms for a uniform, predictable path.
pub fn get_config_dir() -> Result<PathBuf> {
    let mut path = dirs::home_dir().ok_or_else(|| anyhow!("Could not find home directory"))?;
    path.push(".infraweave");
    Ok(path)
}

/// Get the path to the token storage file
pub fn get_token_path() -> Result<PathBuf> {
    let mut path = get_config_dir()?;
    // Ensure directory exists
    std::fs::create_dir_all(&path).ok();
    path.push("tokens.json");
    Ok(path)
}
