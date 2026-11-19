use async_trait::async_trait;
use infraweave_chat::agent::{self, AgentEvent, AgentRequest};
use infraweave_chat::llm::{
    LlmClient, LlmContent, LlmMessage, LlmResponse, LlmUsage, Role, StopReason, ToolDef,
};
use infraweave_tools::{ApiClient, ToolContext};
use std::sync::Mutex;
use tokio::sync::mpsc;

/// Replays a fixed script of responses, one per `converse` call.
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

#[tokio::test]
async fn emits_assistant_turn_as_history_append() {
    let llm = ScriptedLlm {
        script: Mutex::new(vec![LlmResponse {
            content: vec![LlmContent::Text {
                text: "Three modules are published.".to_string(),
            }],
            stop_reason: StopReason::EndTurn,
            usage: LlmUsage::default(),
        }]),
    };
    let ctx = ToolContext::new(ApiClient::new("http://127.0.0.1:1", "t").expect("client"));
    let (tx, mut rx) = mpsc::channel(32);

    agent::run(
        &llm,
        ctx,
        &[],
        AgentRequest {
            system: "sys".to_string(),
            history: vec![],
            user_message: LlmMessage::user_text("what modules are published?"),
        },
        tx,
    )
    .await;

    let mut events = Vec::new();
    while let Some(ev) = rx.recv().await {
        events.push(ev);
    }

    let appended: Vec<&LlmMessage> = events
        .iter()
        .filter_map(|e| match e {
            AgentEvent::HistoryAppend { message } => Some(message),
            _ => None,
        })
        .collect();

    // The user turn and the assistant turn both belong in the transcript.
    assert_eq!(
        appended.len(),
        2,
        "expected user + assistant history entries"
    );
    assert_eq!(appended[0].role, Role::User);
    assert_eq!(appended[1].role, Role::Assistant);
}
