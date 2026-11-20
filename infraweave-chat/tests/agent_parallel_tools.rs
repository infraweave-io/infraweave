use async_trait::async_trait;
use infraweave_chat::agent::{self, AgentEvent, AgentRequest};
use infraweave_chat::llm::{
    LlmClient, LlmContent, LlmMessage, LlmResponse, LlmUsage, Role, StopReason, ToolDef,
};
use infraweave_tools::{ApiClient, Tool, ToolContext, ToolDef as InfraToolDef};
use serde_json::{json, Value};
use std::sync::Mutex;
use std::time::Duration;
use tokio::sync::mpsc;

struct ScriptedLlm {
    script: Mutex<Vec<LlmResponse>>,
}

#[async_trait]
impl LlmClient for ScriptedLlm {
    async fn converse(
        &self,
        _system: &str,
        _messages: &[LlmMessage],
        _tools: &[ToolDef],
    ) -> anyhow::Result<LlmResponse> {
        let mut script = self.script.lock().expect("script lock poisoned");
        if script.is_empty() {
            anyhow::bail!("scripted llm ran out of responses");
        }
        Ok(script.remove(0))
    }
}

/// Sleeps, so sequential execution takes 2x as long as parallel execution.
struct SlowTool;

#[async_trait]
impl Tool for SlowTool {
    fn def(&self) -> InfraToolDef {
        InfraToolDef {
            name: "slow_tool",
            description: "sleeps for 300ms",
            input_schema: json!({"type": "object", "properties": {}}),
        }
    }

    async fn execute(&self, _ctx: &ToolContext, _args: Value) -> anyhow::Result<String> {
        tokio::time::sleep(Duration::from_millis(300)).await;
        Ok("done".to_string())
    }
}

fn two_tool_calls() -> LlmResponse {
    LlmResponse {
        content: vec![
            LlmContent::ToolUse {
                id: "call-1".to_string(),
                name: "slow_tool".to_string(),
                input: json!({}),
            },
            LlmContent::ToolUse {
                id: "call-2".to_string(),
                name: "slow_tool".to_string(),
                input: json!({}),
            },
        ],
        stop_reason: StopReason::ToolUse,
        usage: LlmUsage::default(),
    }
}

#[tokio::test]
async fn executes_tools_in_one_turn_concurrently() {
    let llm = ScriptedLlm {
        script: Mutex::new(vec![
            two_tool_calls(),
            LlmResponse {
                content: vec![LlmContent::Text {
                    text: "both done".to_string(),
                }],
                stop_reason: StopReason::EndTurn,
                usage: LlmUsage::default(),
            },
        ]),
    };
    let ctx = ToolContext::new(ApiClient::new("http://127.0.0.1:1", "t").expect("client"));
    let tools: Vec<Box<dyn Tool>> = vec![Box::new(SlowTool)];
    let (tx, mut rx) = mpsc::channel(64);

    let started = std::time::Instant::now();
    agent::run(
        &llm,
        ctx,
        &tools,
        AgentRequest {
            system: "sys".to_string(),
            history: vec![],
            user_message: LlmMessage::user_text("run both"),
        },
        tx,
    )
    .await;
    let elapsed = started.elapsed();

    let mut events = Vec::new();
    while let Some(ev) = rx.recv().await {
        events.push(ev);
    }

    // Two 300ms tools run concurrently finish well under the 600ms a sequential
    // loop would need.
    assert!(
        elapsed < Duration::from_millis(500),
        "tools appear to run sequentially: took {elapsed:?}"
    );

    let results = events
        .iter()
        .filter(|e| matches!(e, AgentEvent::ToolResult { .. }))
        .count();
    assert_eq!(results, 2, "both tool results should be emitted");
}

#[tokio::test]
async fn returns_all_tool_results_in_a_single_user_message() {
    let llm = ScriptedLlm {
        script: Mutex::new(vec![
            two_tool_calls(),
            LlmResponse {
                content: vec![LlmContent::Text {
                    text: "both done".to_string(),
                }],
                stop_reason: StopReason::EndTurn,
                usage: LlmUsage::default(),
            },
        ]),
    };
    let ctx = ToolContext::new(ApiClient::new("http://127.0.0.1:1", "t").expect("client"));
    let tools: Vec<Box<dyn Tool>> = vec![Box::new(SlowTool)];
    let (tx, mut rx) = mpsc::channel(64);

    agent::run(
        &llm,
        ctx,
        &tools,
        AgentRequest {
            system: "sys".to_string(),
            history: vec![],
            user_message: LlmMessage::user_text("run both"),
        },
        tx,
    )
    .await;

    let mut tool_result_turns = Vec::new();
    while let Some(ev) = rx.recv().await {
        if let AgentEvent::HistoryAppend { message } = ev {
            let is_tool_turn = message.role == Role::User
                && message
                    .content
                    .iter()
                    .any(|c| matches!(c, LlmContent::ToolResult { .. }));
            if is_tool_turn {
                tool_result_turns.push(message);
            }
        }
    }

    // Splitting tool results across several user messages teaches the model to
    // stop making parallel calls. There must be exactly one, holding both.
    assert_eq!(
        tool_result_turns.len(),
        1,
        "tool results must arrive in exactly one user message"
    );
    assert_eq!(tool_result_turns[0].content.len(), 2);
}
