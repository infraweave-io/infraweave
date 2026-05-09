//! Tool-use loop.

use anyhow::Result;
use infraweave_tools::{Tool, ToolContext};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use tokio::sync::mpsc;

use crate::llm::{LlmClient, LlmContent, LlmMessage, Role, StopReason, ToolDef};

const MAX_TOOL_ITERATIONS: usize = 10;

/// Events surfaced to the chat handler. The handler maps these onto SSE frames.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AgentEvent {
    /// LLM has decided to call a tool. Emitted *before* execution so the UI
    /// can show "Looking up modules..." etc.
    ToolCall {
        name: String,
        input: Value,
    },
    /// Tool finished. `output_preview` is truncated; full output is fed back
    /// to the LLM but not necessarily forwarded to the browser.
    ToolResult {
        name: String,
        output_preview: String,
        is_error: bool,
    },
    /// Final assistant text.
    Text {
        text: String,
    },
    /// Iteration cap hit - bail out cleanly.
    Truncated {
        reason: String,
    },
    Done,
    Error {
        message: String,
    },
}

pub struct AgentRequest {
    pub system: String,
    pub history: Vec<LlmMessage>,
    pub user_message: String,
}

pub async fn run(
    llm: &dyn LlmClient,
    tool_ctx: ToolContext,
    tools: &[Box<dyn Tool>],
    req: AgentRequest,
    tx: mpsc::Sender<AgentEvent>,
) {
    if let Err(e) = run_inner(llm, tool_ctx, tools, req, &tx).await {
        // `{:#}` walks the anyhow context chain so the SSE consumer sees the
        // root cause (e.g. Bedrock SDK error) instead of just the wrapper.
        let _ = tx
            .send(AgentEvent::Error {
                message: format!("{e:#}"),
            })
            .await;
    }
    let _ = tx.send(AgentEvent::Done).await;
}

async fn run_inner(
    llm: &dyn LlmClient,
    tool_ctx: ToolContext,
    tools: &[Box<dyn Tool>],
    req: AgentRequest,
    tx: &mpsc::Sender<AgentEvent>,
) -> Result<()> {
    let tool_defs: Vec<ToolDef> = tools
        .iter()
        .map(|t| {
            let d = t.def();
            ToolDef {
                name: d.name.to_string(),
                description: d.description.to_string(),
                input_schema: d.input_schema,
            }
        })
        .collect();
    let by_name: HashMap<String, &Box<dyn Tool>> = tools
        .iter()
        .map(|t| (t.def().name.to_string(), t))
        .collect();

    let mut messages = req.history;
    messages.push(LlmMessage::user_text(req.user_message));

    for iter in 0..MAX_TOOL_ITERATIONS {
        let resp = llm.converse(&req.system, &messages, &tool_defs).await?;

        for block in &resp.content {
            if let LlmContent::Text { text } = block {
                if !text.is_empty() {
                    let _ = tx.send(AgentEvent::Text { text: text.clone() }).await;
                }
            }
        }

        // Record the assistant turn before we attach tool results.
        messages.push(LlmMessage {
            role: Role::Assistant,
            content: resp.content.clone(),
        });

        if resp.stop_reason != StopReason::ToolUse {
            return Ok(());
        }

        let mut tool_result_blocks = Vec::new();
        for block in &resp.content {
            let LlmContent::ToolUse { id, name, input } = block else {
                continue;
            };
            let _ = tx
                .send(AgentEvent::ToolCall {
                    name: name.clone(),
                    input: input.clone(),
                })
                .await;

            let (output, is_error) = match by_name.get(name) {
                Some(tool) => match tool.execute(&tool_ctx, input.clone()).await {
                    Ok(s) => (s, false),
                    Err(e) => (format!("tool execution failed: {e}"), true),
                },
                None => (format!("unknown tool: `{name}`"), true),
            };

            let preview: String = output.chars().take(400).collect();
            let _ = tx
                .send(AgentEvent::ToolResult {
                    name: name.clone(),
                    output_preview: preview,
                    is_error,
                })
                .await;

            tool_result_blocks.push(LlmContent::ToolResult {
                tool_use_id: id.clone(),
                content: output,
                is_error,
            });
        }

        // Tool results are sent back as a single user-role message.
        messages.push(LlmMessage {
            role: Role::User,
            content: tool_result_blocks,
        });

        if iter == MAX_TOOL_ITERATIONS - 1 {
            let _ = tx
                .send(AgentEvent::Truncated {
                    reason: format!("hit MAX_TOOL_ITERATIONS={MAX_TOOL_ITERATIONS}"),
                })
                .await;
        }
    }
    Ok(())
}
