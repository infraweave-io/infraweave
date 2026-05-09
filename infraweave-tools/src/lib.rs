//! Curated, chatbot-friendly tools for InfraWeave.
//!
//! Each tool is intent-shaped (e.g. "debug_deployment", "diff_module_versions")
//! rather than a thin wrapper over a REST endpoint. Tool outputs are concise
//! markdown strings, optimised for LLM consumption rather than raw API JSON.
//!
//! The same registry is consumed by:
//!   - `infraweave-chat` (Bedrock / Anthropic / OpenAI tool use)
//!   - `infraweave-mcp` (stdio MCP server for IDE clients)

mod client;
mod context;
mod tool;
mod tools;

pub use client::ApiClient;
pub use context::ToolContext;
pub use tool::{Tool, ToolDef};
pub use tools::registry;
