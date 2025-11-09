# InfraWeave MCP Server

MCP (Model Context Protocol) server for InfraWeave.

## Overview

A standalone server that:
1. Generates OpenAPI spec in-memory from the `webserver-openapi` crate
2. Starts an embedded webserver on a random localhost port
3. Auto-generates MCP tools from OpenAPI endpoints
4. Translates MCP JSON-RPC calls to HTTP REST calls
5. Communicates via stdio (JSON-RPC over stdin/stdout)

## Usage

Run via the CLI:
```bash
# Start the MCP server
cargo run -p cli -- mcp

# Setup for VS Code
cargo run -p cli -- mcp setup-vscode

# Setup for Claude Desktop
cargo run -p cli -- mcp setup-claude
```

## Security

The server uses process-isolated authentication:
- Random 64-character token generated at startup
- Token stored in memory only (never written to disk or environment)
- Webserver bound to localhost (127.0.0.1)
- Random OS-assigned port to avoid conflicts

## Testing

```bash
# Run the MCP server
cargo run

# Test with JSON-RPC:
echo '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}' | cargo run
```
