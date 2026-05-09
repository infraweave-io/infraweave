//! MCP server entry point.
//!
//! CRITICAL: MCP protocol uses stdio for JSON-RPC communication.
//! - stdout: ONLY for MCP JSON-RPC messages
//! - stderr: ALL logging, debug output
//!
//! Always use `eprintln!()` for logs, never `println!()`.

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    infraweave_mcp::run().await
}
