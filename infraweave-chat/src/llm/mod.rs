//! LLM-provider abstraction.

use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::mpsc;

#[cfg(feature = "bedrock")]
pub mod bedrock;
pub mod stream_assembly;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    User,
    Assistant,
}

/// One block inside a chat message. The same shape covers user input,
/// assistant text, model-issued tool calls, and tool results coming back.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum LlmContent {
    Text {
        text: String,
    },
    /// Model reasoning. Must be echoed back unchanged on later turns - the
    /// `signature` is what lets Bedrock verify the block was not tampered with.
    Reasoning {
        text: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        signature: Option<String>,
    },
    /// Base64-encoded opaque reasoning. Echoed back verbatim; never inspected.
    RedactedReasoning {
        data: String,
    },
    ToolUse {
        id: String,
        name: String,
        input: Value,
    },
    ToolResult {
        tool_use_id: String,
        content: String,
        is_error: bool,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmMessage {
    pub role: Role,
    pub content: Vec<LlmContent>,
}

impl LlmMessage {
    pub fn user_text(text: impl Into<String>) -> Self {
        Self {
            role: Role::User,
            content: vec![LlmContent::Text { text: text.into() }],
        }
    }
}

#[derive(Debug, Clone)]
pub struct ToolDef {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StopReason {
    EndTurn,
    ToolUse,
    MaxTokens,
    Other(String),
}

/// Token accounting for one model call. The cache fields are what prove prompt
/// caching is actually working
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LlmUsage {
    pub input_tokens: i32,
    pub output_tokens: i32,
    pub cache_read_input_tokens: i32,
    pub cache_write_input_tokens: i32,
}

#[derive(Debug, Clone)]
pub struct LlmResponse {
    pub content: Vec<LlmContent>,
    pub stop_reason: StopReason,
    pub usage: LlmUsage,
}

/// Incremental output from a streaming model call. Display only - the assembled
/// `LlmResponse` is what goes into the transcript.
#[derive(Debug, Clone)]
pub enum LlmDelta {
    Text { text: String },
    Reasoning { text: String },
}

#[async_trait]
pub trait LlmClient: Send + Sync {
    async fn converse(
        &self,
        system: &str,
        messages: &[LlmMessage],
        tools: &[ToolDef],
    ) -> Result<LlmResponse>;

    /// Streaming variant. Emits deltas as they arrive and returns the assembled
    /// response. The default implementation falls back to `converse` and emits
    /// one delta per text block, so fakes and non-streaming providers keep
    /// working without change.
    async fn converse_stream(
        &self,
        system: &str,
        messages: &[LlmMessage],
        tools: &[ToolDef],
        tx: &mpsc::Sender<LlmDelta>,
    ) -> Result<LlmResponse> {
        let resp = self.converse(system, messages, tools).await?;
        for block in &resp.content {
            match block {
                LlmContent::Text { text } if !text.is_empty() => {
                    let _ = tx.send(LlmDelta::Text { text: text.clone() }).await;
                }
                LlmContent::Reasoning { text, .. } if !text.is_empty() => {
                    let _ = tx.send(LlmDelta::Reasoning { text: text.clone() }).await;
                }
                _ => {}
            }
        }
        Ok(resp)
    }
}
