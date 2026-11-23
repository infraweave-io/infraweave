use serde_json::Value;
use tokio::sync::mpsc;

use infraweave_chat::{
    agent::{self, AgentEvent, AgentRequest},
    llm::{LlmContent, LlmMessage, LlmResponse, Role},
    system_prompt::SYSTEM_PROMPT,
};
use infraweave_tools::{ApiClient, ToolContext};

use super::fake_llm::{FakeLlm, RecordedCall};
use super::stub_api::StubApi;

/// One end-to-end run of the agent loop against fakes.
pub struct Scenario {
    user_message: String,
    history: Vec<LlmMessage>,
    script: Vec<LlmResponse>,
    routes: Vec<(&'static str, Value)>,
    project: Option<String>,
    region: Option<String>,
    environment: Option<String>,
    track: Option<String>,
}

impl Scenario {
    pub fn new(user_message: &str) -> Self {
        Self {
            user_message: user_message.to_string(),
            history: Vec::new(),
            script: Vec::new(),
            routes: Vec::new(),
            project: None,
            region: None,
            environment: None,
            track: None,
        }
    }

    pub fn with_script(mut self, script: Vec<LlmResponse>) -> Self {
        self.script = script;
        self
    }

    pub fn with_api_route(mut self, path: &'static str, body: Value) -> Self {
        self.routes.push((path, body));
        self
    }

    pub fn with_history(mut self, history: Vec<LlmMessage>) -> Self {
        self.history = history;
        self
    }

    pub fn with_session_defaults(
        mut self,
        project: &str,
        region: &str,
        environment: &str,
        track: &str,
    ) -> Self {
        self.project = Some(project.to_string());
        self.region = Some(region.to_string());
        self.environment = Some(environment.to_string());
        self.track = Some(track.to_string());
        self
    }

    pub async fn run(self) -> ScenarioResult {
        let stub = StubApi::start(self.routes).await;
        let api = ApiClient::new(stub.endpoint(), "test-token").expect("api client builds");

        let mut ctx = ToolContext::new(api);
        if let Some(p) = self.project {
            ctx = ctx.with_project(p);
        }
        if let Some(r) = self.region {
            ctx = ctx.with_region(r);
        }
        if let Some(e) = self.environment {
            ctx = ctx.with_environment(e);
        }
        if let Some(t) = self.track {
            ctx = ctx.with_track(t);
        }

        let llm = FakeLlm::new(self.script);
        let tools = infraweave_tools::registry();
        // Sized so a full MAX_TOOL_ITERATIONS run never blocks on send: events
        // are only drained after `agent::run` returns.
        let (tx, mut rx) = mpsc::channel(256);

        agent::run(
            &llm,
            ctx,
            &tools,
            AgentRequest {
                system: SYSTEM_PROMPT.to_string(),
                history: self.history,
                user_message: LlmMessage::user_text(self.user_message),
            },
            tx,
        )
        .await;

        let mut events = Vec::new();
        while let Some(ev) = rx.recv().await {
            events.push(ev);
        }

        ScenarioResult {
            events,
            calls: llm.calls(),
            requested_paths: stub.requests(),
        }
    }
}

pub struct ScenarioResult {
    pub events: Vec<AgentEvent>,
    pub calls: Vec<RecordedCall>,
    pub requested_paths: Vec<String>,
}

impl ScenarioResult {
    /// Tool names in the order the agent invoked them.
    pub fn tool_calls_made(&self) -> Vec<String> {
        self.events
            .iter()
            .filter_map(|e| match e {
                AgentEvent::ToolCall { name, .. } => Some(name.clone()),
                _ => None,
            })
            .collect()
    }

    /// The arguments a named tool was called with, first call only.
    pub fn tool_input(&self, tool: &str) -> Option<&Value> {
        self.events.iter().find_map(|e| match e {
            AgentEvent::ToolCall { name, input, .. } if name == tool => Some(input),
            _ => None,
        })
    }

    /// The transcript the client would have accumulated from `history_append`.
    pub fn history(&self) -> Vec<LlmMessage> {
        self.events
            .iter()
            .filter_map(|e| match e {
                AgentEvent::HistoryAppend { message } => Some(message.clone()),
                _ => None,
            })
            .collect()
    }

    pub fn assistant_text(&self) -> String {
        self.events
            .iter()
            .filter_map(|e| match e {
                AgentEvent::Text { text } => Some(text.as_str()),
                _ => None,
            })
            .collect()
    }

    pub fn tool_errors(&self) -> Vec<String> {
        self.events
            .iter()
            .filter_map(|e| match e {
                AgentEvent::ToolResult {
                    name,
                    is_error: true,
                    ..
                } => Some(name.clone()),
                _ => None,
            })
            .collect()
    }

    /// Every tool-result block the model would have received, as one flat list.
    pub fn tool_result_blocks(&self) -> Vec<LlmContent> {
        self.history()
            .into_iter()
            .filter(|m| m.role == Role::User)
            .flat_map(|m| m.content)
            .filter(|c| matches!(c, LlmContent::ToolResult { .. }))
            .collect()
    }
}
