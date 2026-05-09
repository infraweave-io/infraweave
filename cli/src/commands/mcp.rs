//! `infraweave mcp` - start the stdio MCP server.
//!
//! Server-side logic lives in the `infraweave-mcp` crate, which is also
//! shipped as a standalone binary and shares the curated `infraweave-tools`
//! registry with `infraweave-chat`. This file only wires the `mcp`
//! subcommand and writes IDE/Claude Desktop config files.

use serde_json::{json, Value};
use std::fs;
use std::path::PathBuf;

/// Run the MCP server on stdio. Blocks until the client disconnects.
///
/// CRITICAL: stdout is reserved for MCP JSON-RPC; never `println!` here.
pub async fn run_mcp_server() -> anyhow::Result<()> {
    infraweave_mcp::run().await
}

fn get_mcp_setup_info() -> anyhow::Result<PathBuf> {
    let exe_path = std::env::current_exe()
        .map_err(|e| anyhow::anyhow!("Failed to get current executable path: {}", e))?;
    println!("Executable: {}", exe_path.display());

    Ok(exe_path)
}

fn vscode_user_dir() -> anyhow::Result<PathBuf> {
    if cfg!(target_os = "macos") {
        Ok(dirs::home_dir()
            .ok_or_else(|| anyhow::anyhow!("Could not determine home directory"))?
            .join("Library/Application Support/Code/User"))
    } else if cfg!(target_os = "linux") {
        Ok(dirs::home_dir()
            .ok_or_else(|| anyhow::anyhow!("Could not determine home directory"))?
            .join(".config/Code/User"))
    } else if cfg!(target_os = "windows") {
        Ok(dirs::config_dir()
            .ok_or_else(|| anyhow::anyhow!("Could not determine config directory"))?
            .join("Code/User"))
    } else {
        Err(anyhow::anyhow!("Unsupported operating system"))
    }
}

/// Configure InfraWeave MCP in VS Code's dedicated user mcp.json.
pub async fn setup_vscode() -> anyhow::Result<()> {
    println!("Setting up InfraWeave MCP server for VS Code...\n");

    let exe_path = get_mcp_setup_info()?;

    let user_dir = vscode_user_dir()?;
    let config_path = user_dir.join("mcp.json");

    println!("\nVS Code MCP config: {}", config_path.display());

    let mut config: Value = if config_path.exists() {
        let content = fs::read_to_string(&config_path)
            .map_err(|e| anyhow::anyhow!("Failed to read mcp.json: {}", e))?;
        serde_json::from_str(&content).unwrap_or_else(|_| json!({}))
    } else {
        if let Some(parent) = config_path.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| anyhow::anyhow!("Failed to create settings directory: {}", e))?;
        }
        json!({})
    };

    let mcp_config = json!({
        "type": "stdio",
        "command": exe_path.to_string_lossy(),
        "args": ["mcp"],
    });

    if config.get("servers").is_none() {
        config["servers"] = json!({});
    }
    config["servers"]["infraweave"] = mcp_config;

    let serialized = serde_json::to_string_pretty(&config)
        .map_err(|e| anyhow::anyhow!("Failed to serialize MCP config: {}", e))?;
    fs::write(&config_path, serialized)
        .map_err(|e| anyhow::anyhow!("Failed to write mcp.json: {}", e))?;

    println!("\nSuccessfully configured InfraWeave MCP server in VS Code.");
    println!("\nAdded at: servers.infraweave");
    println!("\nRestart VS Code to activate.");
    Ok(())
}

/// Configure InfraWeave MCP in Claude Desktop's config file.
pub async fn setup_claude() -> anyhow::Result<()> {
    println!("Setting up InfraWeave MCP server for Claude Desktop...\n");

    let exe_path = get_mcp_setup_info()?;

    let config_path = if cfg!(target_os = "macos") {
        dirs::home_dir()
            .ok_or_else(|| anyhow::anyhow!("Could not determine home directory"))?
            .join("Library/Application Support/Claude/claude_desktop_config.json")
    } else if cfg!(target_os = "linux") {
        dirs::home_dir()
            .ok_or_else(|| anyhow::anyhow!("Could not determine home directory"))?
            .join(".config/Claude/claude_desktop_config.json")
    } else if cfg!(target_os = "windows") {
        dirs::config_dir()
            .ok_or_else(|| anyhow::anyhow!("Could not determine config directory"))?
            .join("Claude/claude_desktop_config.json")
    } else {
        return Err(anyhow::anyhow!("Unsupported operating system"));
    };

    println!("\nClaude Desktop config: {}", config_path.display());

    let mut config: Value = if config_path.exists() {
        let content = fs::read_to_string(&config_path)
            .map_err(|e| anyhow::anyhow!("Failed to read claude_desktop_config.json: {}", e))?;
        serde_json::from_str(&content).unwrap_or_else(|_| json!({}))
    } else {
        if let Some(parent) = config_path.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| anyhow::anyhow!("Failed to create config directory: {}", e))?;
        }
        json!({})
    };

    let mcp_config = json!({
        "type": "stdio",
        "command": exe_path.to_string_lossy(),
        "args": ["mcp"],
    });

    if config.get("mcpServers").is_none() {
        config["mcpServers"] = json!({});
    }
    config["mcpServers"]["infraweave"] = mcp_config;

    let serialized = serde_json::to_string_pretty(&config)
        .map_err(|e| anyhow::anyhow!("Failed to serialize config: {}", e))?;
    fs::write(&config_path, serialized)
        .map_err(|e| anyhow::anyhow!("Failed to write claude_desktop_config.json: {}", e))?;

    println!("\nSuccessfully configured InfraWeave MCP server in Claude Desktop.");
    println!("\nAdded at: mcpServers.infraweave");
    println!("\nRestart Claude Desktop to activate.");
    Ok(())
}
