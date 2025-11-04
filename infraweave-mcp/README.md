# InfraWeave MCP Server

MCP (Model Context Protocol) server for InfraWeave using your existing OpenAPI schema.

## Architecture

This is a **standalone, all-in-one server** that:
1. Generates OpenAPI spec directly in-memory from `webserver-openapi` crate
2. **Starts its own embedded webserver** on a random available port (localhost only, OS-assigned)
3. Auto-generates MCP tools from the OpenAPI endpoints
4. Translates MCP JSON-RPC → HTTP REST calls to its own webserver

**Fully self-contained & secure** - The embedded webserver is bound to localhost only and not accessible from the network. Uses a random OS-assigned port to avoid conflicts.

## Deployment Options

This is embedded within the CLI, simply run `cargo run -p cli -- mcp` to start it.

## How It Works

The MCP server is a **fully standalone, single-binary solution**:
1. Generates OpenAPI spec in-memory from `webserver_openapi::ApiDoc`
2. **Binds to random available port** on `127.0.0.1` (localhost only, OS-assigned)
3. Uses `rmcp-openapi` to convert OpenAPI endpoints to MCP tools
4. Communicates via stdio using JSON-RPC protocol
5. **Proxies tool calls via HTTP to its own embedded webserver** (all localhost traffic)

This architecture ensures:
- **Zero external dependencies** - everything runs in one process
- **Zero port conflicts** - OS assigns a random available port
- **True process isolation** - random token prevents other processes from accessing the API
- **Zero network exposure** - webserver bound to localhost only (127.0.0.1)
- **Zero drift** between MCP tools and actual API
- **No HTTP overhead** for spec loading (in-memory generation)
- **Single binary** deployment - just run `infraweave-mcp`

## Authentication

**Process-isolated security:** The MCP server generates a **random 64-character token** at startup that only exists in process memory (using `OnceLock` static storage). This token:
- Is never written to disk, logs, or environment variables
- Cannot be accessed by child processes
- Cannot be read via `/proc/<pid>/environ` or similar mechanisms
- Is only accessible within the MCP server process memory
- Is used to authenticate HTTP requests to the embedded webserver
- Makes the webserver inaccessible to other processes (even on localhost!)

**Security layers:**
1. **Process-isolated token** - Token stored in static memory (`OnceLock`), not environment variables
2. **Random token auth** - Only the MCP process knows the secret token (64-char random)
3. **Localhost binding** - Webserver bound to `127.0.0.1` (not network accessible)
4. **No external JWT needed** - Completely self-contained authentication

This ensures **true process isolation** - even other local processes owned by the same user cannot access your API!

## Testing

```bash
# Run the MCP server directly
cargo run

# It expects JSON-RPC on stdin, try:
echo '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}' | cargo run
```
