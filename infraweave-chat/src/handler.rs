//! POST /chat - SSE-streaming chat endpoint.

use axum::{
    extract::State,
    http::{HeaderMap, Method, StatusCode, Uri},
    response::{
        sse::{Event, KeepAlive, Sse},
        IntoResponse,
    },
    Json,
};
use futures::stream::Stream;
use infraweave_tools::{ApiClient, Tool, ToolContext};
use serde::Deserialize;
use std::{convert::Infallible, sync::Arc};
use tokio::sync::mpsc;
use tracing::{info, warn};

use crate::{
    agent::{self, AgentEvent, AgentRequest},
    llm::{LlmClient, LlmContent, LlmMessage, Role},
    system_prompt::SYSTEM_PROMPT,
};

#[derive(Clone)]
pub struct AppState {
    pub llm: Arc<dyn LlmClient>,
    pub tools: Arc<Vec<Box<dyn Tool>>>,
    pub api_endpoint: String,
}

#[derive(Debug, Deserialize)]
pub struct ChatRequest {
    pub message: String,
    #[serde(default)]
    pub history: Vec<LlmMessage>,
    #[serde(default)]
    pub project: Option<String>,
    #[serde(default)]
    pub region: Option<String>,
    #[serde(default)]
    pub environment: Option<String>,
    #[serde(default)]
    pub track: Option<String>,
    #[serde(default)]
    pub page_context: Option<String>,
}

pub async fn chat(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<ChatRequest>,
) -> impl IntoResponse {
    info!("received chat request");

    // Extract the user's bearer token from the incoming Authorization header
    // and pass it through to internal-api. This keeps the existing JWT
    // project-level auth in internal-api as the single source of truth -
    // the chat backend doesn't need to re-validate.
    let token = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .unwrap_or("")
        .to_string();

    let api = match ApiClient::new(&state.api_endpoint, token) {
        Ok(c) => c,
        Err(e) => return sse_error(format!("could not build API client: {e}")),
    };

    let mut tool_ctx = ToolContext::new(api);
    if let Some(p) = body.project {
        tool_ctx = tool_ctx.with_project(p);
    }
    if let Some(r) = body.region {
        tool_ctx = tool_ctx.with_region(r);
    }
    if let Some(e) = body.environment {
        tool_ctx = tool_ctx.with_environment(e);
    }
    if let Some(t) = body.track {
        tool_ctx = tool_ctx.with_track(t);
    }

    let (tx, rx) = mpsc::channel::<AgentEvent>(32);

    // Page context rides in the user turn, not the system prompt: the system prompt
    // must stay byte-stable for prompt caching, and Sonnet 5 does not support
    // mid-conversation system messages.
    let user_message = match body
        .page_context
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        Some(ctx) => LlmMessage {
            role: Role::User,
            content: vec![
                LlmContent::Text {
                    text: format!("Page context: {ctx}"),
                },
                LlmContent::Text {
                    text: body.message.clone(),
                },
            ],
        },
        None => LlmMessage::user_text(body.message.clone()),
    };

    let llm = state.llm.clone();
    let tools = state.tools.clone();
    let req = AgentRequest {
        system: SYSTEM_PROMPT.to_string(),
        history: body.history,
        user_message,
    };

    tokio::spawn(async move {
        agent::run(llm.as_ref(), tool_ctx, tools.as_ref(), req, tx).await;
    });

    Sse::new(EventStream { rx }).keep_alive(KeepAlive::default())
}

pub async fn not_found(method: Method, uri: Uri) -> impl IntoResponse {
    warn!(%method, %uri, "request did not match a chat route");
    (StatusCode::NOT_FOUND, "not found")
}

pub async fn non_http_event(method: Method, uri: Uri) -> impl IntoResponse {
    warn!(%method, %uri, "received non-HTTP Lambda event; call POST /chat through API Gateway or a Function URL instead");
    (
        StatusCode::BAD_REQUEST,
        "infraweave-chat expects an HTTP POST /chat request",
    )
}

struct EventStream {
    rx: mpsc::Receiver<AgentEvent>,
}

impl Stream for EventStream {
    type Item = Result<Event, Infallible>;

    fn poll_next(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        match self.rx.poll_recv(cx) {
            std::task::Poll::Ready(Some(ev)) => {
                let json = serde_json::to_string(&ev).unwrap_or_else(|_| "{}".into());
                std::task::Poll::Ready(Some(Ok(Event::default().data(json))))
            }
            std::task::Poll::Ready(None) => std::task::Poll::Ready(None),
            std::task::Poll::Pending => std::task::Poll::Pending,
        }
    }
}

fn sse_error(message: String) -> Sse<EventStream> {
    let (tx, rx) = mpsc::channel(1);
    tokio::spawn(async move {
        let _ = tx.send(AgentEvent::Error { message }).await;
        let _ = tx.send(AgentEvent::Done).await;
    });
    Sse::new(EventStream { rx })
}
