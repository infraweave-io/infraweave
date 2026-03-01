use anyhow::{anyhow, Result};
use std::path::PathBuf;

/// Get the path to the infraweave config directory
/// On macOS: ~/Library/Application Support/infraweave
/// On Linux: ~/.config/infraweave
/// On Windows: %APPDATA%\infraweave
pub fn get_config_dir() -> Result<PathBuf> {
    let mut path = dirs::config_dir().ok_or_else(|| anyhow!("Could not find config directory"))?;
    path.push("infraweave");
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
