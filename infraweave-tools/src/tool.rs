use anyhow::Result;
use async_trait::async_trait;
use serde_json::Value;

use crate::ToolContext;

/// JSON-schema-shaped tool definition. The schema follows the Anthropic
/// `input_schema` format (`{"type":"object", "properties":{...}, "required":[...]}`),
/// which is also what Bedrock Converse and Vertex's Anthropic API accept verbatim.
/// OpenAI/Gemini callers can map this with a small mechanical adapter.
#[derive(Debug, Clone)]
pub struct ToolDef {
    pub name: &'static str,
    pub description: &'static str,
    pub input_schema: Value,
}

/// A single curated tool. `execute` returns a markdown string - narrative,
/// already-summarised output is far more token-efficient than raw API JSON
/// for an LLM to read back.
#[async_trait]
pub trait Tool: Send + Sync {
    fn def(&self) -> ToolDef;
    async fn execute(&self, ctx: &ToolContext, args: Value) -> Result<String>;
}
