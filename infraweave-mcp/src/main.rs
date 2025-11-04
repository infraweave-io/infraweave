use rmcp::ServiceExt;
use rmcp_openapi::Server;
use url::Url;
use utoipa::OpenApi;

/// MCP Server Entry Point
///
/// CRITICAL: MCP protocol uses stdio for JSON-RPC communication.
/// - stdout: ONLY for MCP JSON-RPC messages
/// - stderr: for ALL logging, debug output, etc.
///
/// Always use eprintln!() for logs, never println!()
#[tokio::main]
async fn main() {
    eprintln!("=== InfraWeave MCP Server ===");
    eprintln!("Bundled web server + MCP protocol server");
    eprintln!("");

    // Generate OpenAPI spec directly in memory (no HTTP needed!)
    eprintln!("[OpenAPI] Generating OpenAPI specification...");
    let openapi_spec = webserver_openapi::ApiDoc::openapi();
    let openapi_json =
        serde_json::to_value(&openapi_spec).expect("Failed to serialize OpenAPI spec");

    // Debug: Save spec to file for inspection
    if let Ok(spec_str) = serde_json::to_string_pretty(&openapi_json) {
        if let Err(e) = std::fs::write("/tmp/infraweave-openapi-spec.json", spec_str) {
            eprintln!("[OpenAPI] WARNING: Could not save debug spec: {}", e);
        } else {
            eprintln!("[OpenAPI] Debug: Spec saved to /tmp/infraweave-openapi-spec.json");
        }
    }

    // Debug: Check what's in the spec
    if let Some(paths) = openapi_json.get("paths").and_then(|p| p.as_object()) {
        eprintln!("[OpenAPI] Found {} endpoints in spec", paths.len());
        for (path, _) in paths.iter().take(3) {
            eprintln!("[OpenAPI]   - {}", path);
        }
    } else {
        eprintln!("[OpenAPI] WARNING: No paths found in OpenAPI spec!");
    }

    if let Some(servers) = openapi_json.get("servers") {
        eprintln!("[OpenAPI] Servers: {:?}", servers);
    } else {
        eprintln!("[OpenAPI] WARNING: No servers defined in OpenAPI spec!");
    }

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
        Ok(url) => {
            eprintln!("[MCP] Parsed base URL: {}", url);
            url
        }
        Err(e) => {
            eprintln!("[MCP] ERROR: Failed to parse API URL: {}", e);
            return;
        }
    };

    // Get JWT token if provided and create default headers
    let default_headers = {
        let mut headers = reqwest::header::HeaderMap::new();

        // Use the internal random token for authentication
        match reqwest::header::HeaderValue::from_str(&format!("Bearer {}", secret_token)) {
            Ok(value) => {
                headers.insert(reqwest::header::AUTHORIZATION, value);
                eprintln!("[MCP] ✓ Using internal random token authentication (process-isolated)");
                Some(headers)
            }
            Err(e) => {
                eprintln!("[MCP] ERROR: Invalid internal token: {}", e);
                return;
            }
        }
    };

    // Create MCP server
    eprintln!(
        "[MCP] Creating Server with {} endpoints",
        openapi_json
            .get("paths")
            .and_then(|p| p.as_object())
            .map(|o| o.len())
            .unwrap_or(0)
    );

    // Debug: Log the actual JSON being passed
    eprintln!(
        "[MCP] OpenAPI JSON keys: {:?}",
        openapi_json
            .as_object()
            .map(|o| o.keys().collect::<Vec<_>>())
    );
    eprintln!("[MCP] OpenAPI version: {:?}", openapi_json.get("openapi"));
    eprintln!("[MCP] Server base URL: {}", base_url);
    eprintln!("[MCP] Has default headers: {}", default_headers.is_some());

    let mut server = Server::new(
        openapi_json.clone(), // Clone so we can still inspect it
        base_url,
        default_headers,
        None,  // parameter_filter
        false, // skip_parameter_descriptions
        false, // skip_unspecified_query_parameters
    );

    eprintln!("[MCP] Server instance created, loading OpenAPI spec...");

    // CRITICAL: Must call load_openapi_spec() to actually parse and generate tools!
    server
        .load_openapi_spec()
        .expect("Failed to load OpenAPI spec into MCP server");

    eprintln!("[MCP] ✓ OpenAPI spec loaded successfully, tools generated");

    eprintln!("");
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
            } else {
                eprintln!("[MCP] Server exited normally");
            }
        }
        Err(e) => {
            eprintln!("[MCP] ERROR: MCP server initialization error: {}", e);
        }
    }

    // Note: webserver task will be terminated when process exits
}
