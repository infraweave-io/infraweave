//! Tool-use loop.

use anyhow::Result;
use infraweave_tools::{Tool, ToolContext};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use tokio::sync::mpsc;

use crate::llm::{LlmClient, LlmContent, LlmDelta, LlmMessage, Role, StopReason, ToolDef};

const MAX_TOOL_ITERATIONS: usize = 10;

/// Events surfaced to the chat handler. The handler maps these onto SSE frames.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AgentEvent {
    /// LLM decided to call a tool. Emitted *before* execution. `id` correlates
    /// this with the matching `ToolResult`.
    ToolCall {
        id: String,
        name: String,
        input: Value,
    },
    /// Tool finished. `output_preview` is truncated for display; the full output
    /// goes back to the model regardless.
    ToolResult {
        id: String,
        name: String,
        output_preview: String,
        is_error: bool,
    },
    /// Assistant text. Display only.
    Text {
        text: String,
    },
    /// Summarised model reasoning. Display only.
    Reasoning {
        text: String,
    },
    /// The canonical transcript entry the agent appended to its own history.
    /// Clients store these verbatim and send them back as `history` next turn.
    /// Treat the payload as opaque - never construct or edit one client-side.
    HistoryAppend {
        message: LlmMessage,
    },
    /// Token accounting for one model call.
    Usage {
        input_tokens: i32,
        output_tokens: i32,
        cache_read_input_tokens: i32,
        cache_write_input_tokens: i32,
    },
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
    /// The full user turn, already composed (page context + the user's words).
    pub user_message: LlmMessage,
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
    let by_name: HashMap<String, &dyn Tool> = tools
        .iter()
        .map(|t| (t.def().name.to_string(), t.as_ref()))
        .collect();

    let mut messages = req.history;
    messages.push(req.user_message.clone());
    let _ = tx
        .send(AgentEvent::HistoryAppend {
            message: req.user_message,
        })
        .await;

    for iter in 0..MAX_TOOL_ITERATIONS {
        let (delta_tx, mut delta_rx) = mpsc::channel::<LlmDelta>(64);
        let forward_tx = tx.clone();
        let forwarder = tokio::spawn(async move {
            while let Some(delta) = delta_rx.recv().await {
                let event = match delta {
                    LlmDelta::Text { text } => AgentEvent::Text { text },
                    LlmDelta::Reasoning { text } => AgentEvent::Reasoning { text },
                };
                let _ = forward_tx.send(event).await;
            }
        });

        let resp = llm
            .converse_stream(&req.system, &messages, &tool_defs, &delta_tx)
            .await;
        drop(delta_tx);
        let _ = forwarder.await;
        let resp = resp?;

        let _ = tx
            .send(AgentEvent::Usage {
                input_tokens: resp.usage.input_tokens,
                output_tokens: resp.usage.output_tokens,
                cache_read_input_tokens: resp.usage.cache_read_input_tokens,
                cache_write_input_tokens: resp.usage.cache_write_input_tokens,
            })
            .await;

        // Record the assistant turn before we attach tool results.
        let assistant_turn = LlmMessage {
            role: Role::Assistant,
            content: resp.content.clone(),
        };
        messages.push(assistant_turn.clone());
        let _ = tx
            .send(AgentEvent::HistoryAppend {
                message: assistant_turn,
            })
            .await;

        if resp.stop_reason != StopReason::ToolUse {
            return Ok(());
        }

        // Collect every tool call in this assistant turn before executing any of
        // them, so they can run concurrently.
        let calls: Vec<(String, String, Value)> = resp
            .content
            .iter()
            .filter_map(|block| match block {
                LlmContent::ToolUse { id, name, input } => {
                    Some((id.clone(), name.clone(), input.clone()))
                }
                _ => None,
            })
            .collect();

        for (id, name, input) in &calls {
            let _ = tx
                .send(AgentEvent::ToolCall {
                    id: id.clone(),
                    name: name.clone(),
                    input: input.clone(),
                })
                .await;
        }

        let outcomes = futures::future::join_all(calls.iter().map(|(_, name, input)| {
            let ctx = tool_ctx.clone();
            let tool = by_name.get(name).copied();
            let name = name.clone();
            let input = input.clone();
            async move {
                match tool {
                    Some(tool) => match tool.execute(&ctx, input).await {
                        Ok(output) => (output, false),
                        Err(e) => (format!("tool execution failed: {e}"), true),
                    },
                    None => (format!("unknown tool: `{name}`"), true),
                }
            }
        }))
        .await;

        let mut tool_result_blocks = Vec::with_capacity(calls.len());
        for ((id, name, _), (output, is_error)) in calls.iter().zip(outcomes) {
            let preview: String = output.chars().take(400).collect();
            let _ = tx
                .send(AgentEvent::ToolResult {
                    id: id.clone(),
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
        let tool_turn = LlmMessage {
            role: Role::User,
            content: tool_result_blocks,
        };
        messages.push(tool_turn.clone());
        let _ = tx
            .send(AgentEvent::HistoryAppend { message: tool_turn })
            .await;

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
