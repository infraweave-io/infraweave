use anyhow::{anyhow, Result};
use async_trait::async_trait;
use std::sync::Mutex;

use infraweave_chat::llm::{LlmClient, LlmMessage, LlmResponse, ToolDef};

/// One `converse` call as the agent made it.
#[derive(Debug, Clone)]
pub struct RecordedCall {
    pub system: String,
    pub messages: Vec<LlmMessage>,
    pub tool_names: Vec<String>,
}

/// Replays a fixed script of responses, one per call, and records every request.
///
/// This is what makes the suite deterministic: the "model" makes exactly the
/// tool calls the scenario says it makes, so the assertions are about the
/// agent's plumbing, never about model judgement.
pub struct FakeLlm {
    script: Mutex<Vec<LlmResponse>>,
    calls: Mutex<Vec<RecordedCall>>,
}

impl FakeLlm {
    pub fn new(script: Vec<LlmResponse>) -> Self {
        Self {
            script: Mutex::new(script),
            calls: Mutex::new(Vec::new()),
        }
    }

    pub fn calls(&self) -> Vec<RecordedCall> {
        self.calls.lock().expect("calls lock poisoned").clone()
    }
}

#[async_trait]
impl LlmClient for FakeLlm {
    async fn converse(
        &self,
        system: &str,
        messages: &[LlmMessage],
        tools: &[ToolDef],
    ) -> Result<LlmResponse> {
        let call_count = {
            let mut calls = self.calls.lock().expect("calls lock poisoned");
            calls.push(RecordedCall {
                system: system.to_string(),
                messages: messages.to_vec(),
                tool_names: tools.iter().map(|t| t.name.clone()).collect(),
            });
            calls.len()
        };

        let mut script = self.script.lock().expect("script lock poisoned");
        if script.is_empty() {
            return Err(anyhow!(
                "fake llm ran out of scripted responses after {call_count} call(s)"
            ));
        }
        Ok(script.remove(0))
    }
}
