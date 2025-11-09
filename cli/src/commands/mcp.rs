use rmcp::ServiceExt;
use rmcp_openapi::Server;
use url::Url;
use utoipa::OpenApi;

/// MCP Server Command
///
/// Starts the Model Context Protocol server for AI tool integration.
/// The server runs a fully self-contained architecture:
/// - Generates OpenAPI spec in-memory
/// - Starts embedded webserver on random localhost port
/// - Provides MCP protocol interface via stdio
///
/// CRITICAL: MCP protocol uses stdio for JSON-RPC communication.
/// - stdout: ONLY for MCP JSON-RPC messages
/// - stderr: for ALL logging, debug output, etc.
pub async fn run_mcp_server() -> anyhow::Result<()> {
    eprintln!("=== InfraWeave MCP Server ===");
    eprintln!("Bundled web server + MCP protocol server");
    eprintln!("");

    // Generate OpenAPI spec directly in memory (no HTTP needed!)
    eprintln!("[OpenAPI] Generating OpenAPI specification...");
    let openapi_spec = webserver_openapi::ApiDoc::openapi();
    let openapi_json =
        serde_json::to_value(&openapi_spec).expect("Failed to serialize OpenAPI spec");

    eprintln!("[OpenAPI] ✓ OpenAPI spec generated in-memory");
    eprintln!("");

    // Generate a secure random token for internal authentication
    // This ensures only THIS MCP server instance can access the embedded webserver
    use rand::Rng;
    let secret_token: String = rand::thread_rng()
        .sample_iter(&rand::distributions::Alphanumeric)
        .take(64)
        .map(char::from)
        .collect();

    eprintln!("[Security] Generated random auth token for internal use only");

    // Set the token in process-isolated storage (NOT environment variables!)
    // This ensures ONLY this process can access the token - no child processes,
    // no /proc/<pid>/environ leaks, no other processes owned by same user
    webserver_openapi::set_internal_token(secret_token.clone());

    // Bind to a random available port (OS assigns it)
    // We keep the listener and pass it to the webserver to avoid race conditions
    eprintln!("[WebServer] Finding available port...");
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("Failed to bind to any port");

    let actual_port = listener
        .local_addr()
        .expect("Failed to get local address")
        .port();

    eprintln!("[WebServer] Starting on 127.0.0.1:{}...", actual_port);

    // Start the webserver with the existing listener (no race condition!)
    let _webserver_handle = tokio::spawn(async move {
        // Disable UI (Swagger/ReDoc) but ENABLE auth with random token in MCP mode
        // This ensures ONLY this MCP process can access the webserver
        if let Err(e) = webserver_openapi::run_server_with_listener(listener, false, false).await {
            eprintln!("[WebServer] ERROR: {}", e);
            std::process::exit(1);
        }
    });

    // Give the webserver a moment to start up
    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

    eprintln!(
        "[WebServer] ✓ API server running on 127.0.0.1:{} (localhost only, token-protected)",
        actual_port
    );
    eprintln!("");

    // Build API URL using the discovered port
    let api_url = format!("http://localhost:{}", actual_port);

    eprintln!("[MCP] Initializing MCP server...");
    eprintln!("[MCP] API base URL: {}", api_url);

    // Parse base URL - rmcp-openapi uses this to override servers in the spec
    let base_url = match Url::parse(&api_url) {
        Ok(url) => url,
        Err(e) => {
            eprintln!("[MCP] ERROR: Failed to parse API URL: {}", e);
            return Err(anyhow::anyhow!("Failed to parse API URL: {}", e));
        }
    };

    // Get JWT token if provided and create default headers
    let default_headers = {
        let mut headers = reqwest::header::HeaderMap::new();

        // Use the internal random token for authentication
        match reqwest::header::HeaderValue::from_str(&format!("Bearer {}", secret_token)) {
            Ok(value) => {
                headers.insert(reqwest::header::AUTHORIZATION, value);
                Some(headers)
            }
            Err(e) => {
                eprintln!("[MCP] ERROR: Invalid internal token: {}", e);
                return Err(anyhow::anyhow!("Invalid internal token: {}", e));
            }
        }
    };

    // Create MCP server
    let mut server = Server::new(
        openapi_json.clone(), // Clone so we can still inspect it
        base_url,
        default_headers,
        None,  // parameter_filter
        false, // skip_parameter_descriptions
        false, // skip_unspecified_query_parameters
    );

    // CRITICAL: Must call load_openapi_spec() to actually parse and generate tools!
    server
        .load_openapi_spec()
        .expect("Failed to load OpenAPI spec into MCP server");

    eprintln!("[MCP] ✓ MCP server initialized");
    eprintln!("[MCP] Using direct in-memory OpenAPI spec (no HTTP overhead)");
    eprintln!("[MCP] Protocol: stdio (compatible with Claude Desktop, Cline, etc.)");
    eprintln!("");
    eprintln!("=== Server Ready ===");
    eprintln!("Waiting for MCP client connections...");
    eprintln!("");

    // Run the MCP server on stdio (for Claude Desktop, Cline, etc.)
    let transport = (tokio::io::stdin(), tokio::io::stdout());

    match server.serve(transport).await {
        Ok(running_service) => {
            if let Err(e) = running_service.waiting().await {
                eprintln!("[MCP] ERROR: MCP server error while running: {}", e);
                return Err(anyhow::anyhow!("MCP server error: {}", e));
            } else {
                eprintln!("[MCP] Server exited normally");
            }
        }
        Err(e) => {
            eprintln!("[MCP] ERROR: MCP server initialization error: {}", e);
            return Err(anyhow::anyhow!("MCP server initialization error: {}", e));
        }
    }

    Ok(())
}

/// Common setup logic for MCP server configuration
fn get_mcp_setup_info() -> anyhow::Result<(std::path::PathBuf, Option<String>, Option<String>)> {
    // Get the current executable path
    let exe_path = std::env::current_exe()
        .map_err(|e| anyhow::anyhow!("Failed to get current executable path: {}", e))?;

    println!("📍 Executable: {}", exe_path.display());

    // Get AWS credentials from environment
    let aws_profile = std::env::var("AWS_PROFILE").ok();
    let aws_region = std::env::var("AWS_REGION").ok();

    if let Some(ref profile) = aws_profile {
        println!("🔑 AWS Profile: {}", profile);
    } else {
        println!("⚠️  AWS_PROFILE not set in environment");
    }

    if let Some(ref region) = aws_region {
        println!("🌍 AWS Region: {}", region);
    } else {
        println!("⚠️  AWS_REGION not set in environment");
    }

    Ok((exe_path, aws_profile, aws_region))
}

/// Build environment variables map for MCP configuration
fn build_env_vars(
    aws_profile: Option<String>,
    aws_region: Option<String>,
) -> serde_json::Map<String, serde_json::Value> {
    use serde_json::json;

    let mut env_vars = serde_json::Map::new();

    if let Some(profile) = aws_profile {
        env_vars.insert("AWS_PROFILE".to_string(), json!(profile));
    }

    if let Some(region) = aws_region {
        env_vars.insert("AWS_REGION".to_string(), json!(region));
    }

    env_vars
}

/// Setup MCP server in VS Code settings
///
/// Configures the InfraWeave MCP server in VS Code's global settings.json
/// Automatically detects:
/// - Current executable path
/// - AWS_PROFILE and AWS_REGION from environment
pub async fn setup_vscode() -> anyhow::Result<()> {
    use serde_json::{json, Value};
    use std::fs;

    println!("🔧 Setting up InfraWeave MCP server for VS Code...\n");

    let (exe_path, aws_profile, aws_region) = get_mcp_setup_info()?;
    let has_aws_config = aws_profile.is_some() || aws_region.is_some();

    // Find VS Code settings path
    let settings_path = if cfg!(target_os = "macos") {
        dirs::home_dir()
            .ok_or_else(|| anyhow::anyhow!("Could not determine home directory"))?
            .join("Library/Application Support/Code/User/settings.json")
    } else if cfg!(target_os = "linux") {
        dirs::home_dir()
            .ok_or_else(|| anyhow::anyhow!("Could not determine home directory"))?
            .join(".config/Code/User/settings.json")
    } else if cfg!(target_os = "windows") {
        dirs::config_dir()
            .ok_or_else(|| anyhow::anyhow!("Could not determine config directory"))?
            .join("Code/User/settings.json")
    } else {
        return Err(anyhow::anyhow!("Unsupported operating system"));
    };

    println!("\n📁 VS Code settings: {}", settings_path.display());

    // Read existing settings or create new
    let mut settings: Value = if settings_path.exists() {
        let content = fs::read_to_string(&settings_path)
            .map_err(|e| anyhow::anyhow!("Failed to read settings.json: {}", e))?;

        serde_json::from_str(&content).unwrap_or_else(|_| json!({}))
    } else {
        // Create parent directory if needed
        if let Some(parent) = settings_path.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| anyhow::anyhow!("Failed to create settings directory: {}", e))?;
        }
        json!({})
    };

    // Build MCP server configuration
    let env_vars = build_env_vars(aws_profile, aws_region);

    let mcp_config = json!({
        "type": "stdio",
        "command": exe_path.to_string_lossy(),
        "args": ["mcp"],
        "env": env_vars
    });

    // Ensure mcp.servers exists
    if !settings.get("mcp").is_some() {
        settings["mcp"] = json!({});
    }

    if !settings["mcp"].get("servers").is_some() {
        settings["mcp"]["servers"] = json!({});
    }

    // Add/update infraweave server
    settings["mcp"]["servers"]["infraweave"] = mcp_config;

    // Clean up any legacy "mcpServers" entries (Claude Desktop format in VS Code settings)
    if settings.get("mcpServers").is_some() {
        if let Some(mcp_servers) = settings["mcpServers"].as_object() {
            if mcp_servers.contains_key("infraweave") {
                println!(
                    "\n🧹 Removing legacy 'mcpServers.infraweave' entry (Claude Desktop format)"
                );
                settings["mcpServers"]
                    .as_object_mut()
                    .unwrap()
                    .remove("infraweave");

                // Remove the entire mcpServers section if it's now empty
                if settings["mcpServers"].as_object().unwrap().is_empty() {
                    settings.as_object_mut().unwrap().remove("mcpServers");
                }
            }
        }
    }

    // Write updated settings
    let settings_str = serde_json::to_string_pretty(&settings)
        .map_err(|e| anyhow::anyhow!("Failed to serialize settings: {}", e))?;

    fs::write(&settings_path, settings_str)
        .map_err(|e| anyhow::anyhow!("Failed to write settings.json: {}", e))?;

    println!("\n✅ Successfully configured InfraWeave MCP server in VS Code!");
    println!("\n📝 Configuration added:");
    println!("   mcp.servers.infraweave");
    println!("\n🔄 Restart VS Code to activate the MCP server");

    if !has_aws_config {
        println!("\n💡 Tip: Set AWS_PROFILE and AWS_REGION before running this command to");
        println!("   automatically include them in the VS Code configuration.");
    }

    Ok(())
}

/// Setup MCP server in Claude Desktop settings
///
/// Configures the InfraWeave MCP server in Claude Desktop's config file.
/// Automatically detects:
/// - Current executable path
/// - AWS_PROFILE and AWS_REGION from environment
pub async fn setup_claude() -> anyhow::Result<()> {
    use serde_json::{json, Value};
    use std::fs;

    println!("🔧 Setting up InfraWeave MCP server for Claude Desktop...\n");

    let (exe_path, aws_profile, aws_region) = get_mcp_setup_info()?;
    let has_aws_config = aws_profile.is_some() || aws_region.is_some();

    // Find Claude Desktop config path
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

    println!("\n📁 Claude Desktop config: {}", config_path.display());

    // Read existing config or create new
    let mut config: Value = if config_path.exists() {
        let content = fs::read_to_string(&config_path)
            .map_err(|e| anyhow::anyhow!("Failed to read claude_desktop_config.json: {}", e))?;

        serde_json::from_str(&content).unwrap_or_else(|_| json!({}))
    } else {
        // Create parent directory if needed
        if let Some(parent) = config_path.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| anyhow::anyhow!("Failed to create config directory: {}", e))?;
        }
        json!({})
    };

    // Build MCP server configuration for Claude Desktop
    let env_vars = build_env_vars(aws_profile, aws_region);

    // Claude Desktop format (no "type" field, simpler structure)
    let mcp_config = json!({
        "command": exe_path.to_string_lossy(),
        "args": ["mcp"],
        "env": env_vars
    });

    // Ensure mcpServers exists
    if !config.get("mcpServers").is_some() {
        config["mcpServers"] = json!({});
    }

    // Add/update infraweave server
    config["mcpServers"]["infraweave"] = mcp_config;

    // Write updated config
    let config_str = serde_json::to_string_pretty(&config)
        .map_err(|e| anyhow::anyhow!("Failed to serialize config: {}", e))?;

    fs::write(&config_path, config_str)
        .map_err(|e| anyhow::anyhow!("Failed to write claude_desktop_config.json: {}", e))?;

    println!("\n✅ Successfully configured InfraWeave MCP server in Claude Desktop!");
    println!("\n📝 Configuration added:");
    println!("   mcpServers.infraweave");
    println!("\n🔄 Restart Claude Desktop to activate the MCP server");

    if !has_aws_config {
        println!("\n💡 Tip: Set AWS_PROFILE and AWS_REGION before running this command to");
        println!("   automatically include them in the Claude Desktop configuration.");
    }

    Ok(())
}
