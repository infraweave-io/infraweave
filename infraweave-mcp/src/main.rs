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

    // Generate OpenAPI spec directly in memory
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

    // Set the token in process-isolated storage
    // This ensures ONLY this process can access the token - no child processes,
    // no /proc/<pid>/environ leaks, no other processes owned by same user
    webserver_openapi::set_internal_token(secret_token.clone());

    // Bind to random available port on localhost
    eprintln!("[WebServer] Finding available port...");
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("Failed to bind to any port");

    let actual_port = listener
        .local_addr()
        .expect("Failed to get local address")
        .port();

    eprintln!("[WebServer] Starting on 127.0.0.1:{}...", actual_port);

    // Start webserver (no UI, token-protected)
    let _webserver_handle = tokio::spawn(async move {
        if let Err(e) = webserver_openapi::run_server_with_listener(listener, false, false).await {
            eprintln!("[WebServer] ERROR: {}", e);
            std::process::exit(1);
        }
    });

    // Wait for webserver startup
    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

    eprintln!(
        "[WebServer] ✓ API server running on 127.0.0.1:{} (localhost only, token-protected)",
        actual_port
    );
    eprintln!("");

    let api_url = format!("http://localhost:{}", actual_port);

    eprintln!("[MCP] Initializing MCP server...");
    eprintln!("[MCP] API base URL: {}", api_url);

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

    // Create auth headers with internal token
    let default_headers = {
        let mut headers = reqwest::header::HeaderMap::new();

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

    let mut server = Server::new(
        openapi_json.clone(),
        base_url,
        default_headers,
        None,
        false,
        false,
    );

    server
        .load_openapi_spec()
        .expect("Failed to load OpenAPI spec into MCP server");

    eprintln!("[MCP] ✓ MCP server initialized");
    eprintln!("[MCP] Using direct in-memory OpenAPI spec (no HTTP overhead)");
    eprintln!("[MCP] Protocol: stdio (compatible with Claude Desktop, Cline, etc.)");
    eprintln!("");
    eprintln!("=== Server Ready ===");
    eprintln!("");

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
}
