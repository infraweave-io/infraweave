mod support;

use infraweave_chat::agent::AgentEvent;
use infraweave_chat::llm::{LlmContent, LlmMessage, LlmResponse, LlmUsage, Role, StopReason};
use pretty_assertions::assert_eq;
use serde_json::json;
use support::fake_llm::RecordedCall;
use support::fixtures::{one_project, PROJECTS};
use support::scenario::Scenario;

fn text_response(text: &str) -> LlmResponse {
    LlmResponse {
        content: vec![LlmContent::Text {
            text: text.to_string(),
        }],
        stop_reason: StopReason::EndTurn,
        usage: LlmUsage::default(),
    }
}

fn tool_response(calls: Vec<(&str, &str, serde_json::Value)>) -> LlmResponse {
    LlmResponse {
        content: calls
            .into_iter()
            .map(|(id, name, input)| LlmContent::ToolUse {
                id: id.to_string(),
                name: name.to_string(),
                input,
            })
            .collect(),
        stop_reason: StopReason::ToolUse,
        usage: LlmUsage::default(),
    }
}

#[tokio::test]
async fn a_failing_tool_still_returns_a_tool_result_marked_as_an_error() {
    // No route registered, so the stub 404s and list_projects fails.
    let result = Scenario::new("list the projects")
        .allow_unmatched_requests()
        .with_script(vec![
            tool_response(vec![("call-1", "list_projects", json!({}))]),
            text_response("I could not reach the project list."),
        ])
        .run()
        .await;

    assert_eq!(result.tool_errors(), vec!["list_projects".to_string()]);

    // The critical part: the model must still receive a result block. Dropping
    // it leaves a dangling tool_use and the next request is malformed.
    let blocks = result.tool_result_blocks();
    assert_eq!(blocks.len(), 1);
    let LlmContent::ToolResult {
        tool_use_id,
        is_error,
        ..
    } = &blocks[0]
    else {
        panic!("expected a tool result block");
    };
    assert_eq!(tool_use_id, "call-1");
    assert!(is_error);

    // And the next request actually carries it.
    assert_eq!(
        result_ids_sent_on_call(&result.calls, 1),
        vec![("call-1", true)]
    );
}

#[tokio::test]
async fn an_unknown_tool_is_reported_rather_than_silently_skipped() {
    let result = Scenario::new("do something exotic")
        .with_script(vec![
            tool_response(vec![("call-1", "teleport_deployment", json!({}))]),
            text_response("That is not something I can do."),
        ])
        .run()
        .await;

    let blocks = result.tool_result_blocks();
    assert_eq!(blocks.len(), 1);
    let LlmContent::ToolResult {
        content, is_error, ..
    } = &blocks[0]
    else {
        panic!("expected a tool result block");
    };
    assert!(is_error);
    assert!(content.contains("unknown tool"));
    assert_eq!(
        result_ids_sent_on_call(&result.calls, 1),
        vec![("call-1", true)]
    );
}

#[tokio::test]
async fn parallel_tool_calls_produce_one_user_message_holding_every_result() {
    let result = Scenario::new("list projects and modules")
        .with_api_route(PROJECTS, one_project(&["us-west-2"]))
        .with_api_route("/api/v1/modules", json!([]))
        .with_script(vec![
            tool_response(vec![
                ("call-1", "list_projects", json!({})),
                ("call-2", "list_modules", json!({ "track": "stable" })),
            ]),
            text_response("Done."),
        ])
        .run()
        .await;

    assert!(result.tool_errors().is_empty(), "fixtures must deserialize");

    let tool_turns: Vec<LlmMessage> = result
        .history()
        .into_iter()
        .filter(|m| {
            m.role == Role::User
                && m.content
                    .iter()
                    .any(|c| matches!(c, LlmContent::ToolResult { .. }))
        })
        .collect();

    // Splitting results across messages teaches the model to stop calling tools
    // in parallel. There must be exactly one message holding both results.
    assert_eq!(tool_turns.len(), 1);
    assert_eq!(tool_turns[0].content.len(), 2);

    // Both results reach the model in one turn, paired to their calls in order.
    assert_eq!(
        result_ids_sent_on_call(&result.calls, 1),
        vec![("call-1", false), ("call-2", false)]
    );
}

#[tokio::test]
async fn the_transcript_alternates_user_and_assistant_turns() {
    let result = Scenario::new("list the projects")
        .with_api_route(PROJECTS, one_project(&["us-west-2"]))
        .with_script(vec![
            tool_response(vec![("call-1", "list_projects", json!({}))]),
            text_response("One project."),
        ])
        .run()
        .await;

    let roles: Vec<Role> = result.history().into_iter().map(|m| m.role).collect();

    // user question -> assistant tool_use -> user tool_result -> assistant answer
    assert_eq!(
        roles,
        vec![Role::User, Role::Assistant, Role::User, Role::Assistant]
    );
}

#[tokio::test]
async fn replayed_history_is_sent_back_to_the_model_verbatim() {
    let prior = vec![
        LlmMessage::user_text("what modules exist?"),
        LlmMessage {
            role: Role::Assistant,
            content: vec![LlmContent::Text {
                text: "Two modules.".to_string(),
            }],
        },
    ];

    let result = Scenario::new("and which is newest?")
        .with_history(prior.clone())
        .with_script(vec![text_response("The s3bucket-simple module.")])
        .run()
        .await;

    // The agent must prepend the replayed history, not re-derive or summarise it.
    let sent = &result.calls[0].messages;
    assert_eq!(sent.len(), 3, "prior turns plus the new question");
    assert_eq!(as_json(&sent[..2]), as_json(&prior));
    assert_eq!(
        as_json(&sent[2..]),
        as_json(&[LlmMessage::user_text("and which is newest?")])
    );
}

#[tokio::test]
async fn turn_n_history_round_trips_into_a_well_formed_turn_n_plus_1_request() {
    let turn_one = Scenario::new("list the projects")
        .with_api_route(PROJECTS, one_project(&["us-west-2"]))
        .with_script(vec![
            tool_response(vec![("call-1", "list_projects", json!({}))]),
            text_response("One project."),
        ])
        .run()
        .await;

    // What the browser does with `history_append`: store the JSON, send it back.
    let stored = serde_json::to_string(&turn_one.history()).expect("history serializes");
    let replayed: Vec<LlmMessage> = serde_json::from_str(&stored).expect("history parses");

    let turn_two = Scenario::new("which regions?")
        .with_history(replayed)
        .with_script(vec![text_response("us-west-2.")])
        .run()
        .await;

    let mut expected = turn_one.history();
    expected.push(LlmMessage::user_text("which regions?"));
    assert_eq!(as_json(&turn_two.calls[0].messages), as_json(&expected));
}

#[tokio::test]
async fn a_tool_turn_emits_events_in_contract_order() {
    let result = Scenario::new("list the projects")
        .with_api_route(PROJECTS, one_project(&["us-west-2"]))
        .with_script(vec![
            tool_response(vec![("call-1", "list_projects", json!({}))]),
            text_response("One project."),
        ])
        .run()
        .await;

    let labels: Vec<String> = result.events.iter().map(event_label).collect();
    assert_eq!(
        labels,
        vec![
            "history_append:user",
            "usage",
            "history_append:assistant",
            "tool_call:call-1",
            "tool_result:call-1",
            "history_append:user",
            "text",
            "usage",
            "history_append:assistant",
            "done",
        ]
    );
}

#[tokio::test]
async fn the_iteration_cap_truncates_cleanly_instead_of_looping_forever() {
    // Always asks for another tool call; the cap is what stops it.
    let ids: Vec<String> = (0..12).map(|i| format!("call-{i}")).collect();
    let script: Vec<LlmResponse> = ids
        .iter()
        .map(|id| tool_response(vec![(id.as_str(), "list_projects", json!({}))]))
        .collect();

    let result = Scenario::new("loop forever")
        .with_api_route(PROJECTS, one_project(&["us-west-2"]))
        .with_script(script)
        .allow_unused_script()
        .run()
        .await;

    assert!(
        result
            .events
            .iter()
            .any(|e| matches!(e, AgentEvent::Truncated { .. })),
        "hitting MAX_TOOL_ITERATIONS must emit a Truncated event"
    );
    assert_eq!(result.tool_calls_made().len(), 10, "MAX_TOOL_ITERATIONS");
}

#[tokio::test]
async fn the_system_prompt_is_byte_identical_across_iterations() {
    let result = Scenario::new("list the projects")
        .with_api_route(PROJECTS, one_project(&["us-west-2"]))
        .with_script(vec![
            tool_response(vec![("call-1", "list_projects", json!({}))]),
            text_response("One project."),
        ])
        .run()
        .await;

    // Prompt caching is a prefix match. A system prompt that varies between
    // iterations silently destroys the cache hit rate.
    assert_eq!(result.calls.len(), 2);
    assert_eq!(result.calls[0].system, result.calls[1].system);
}

#[tokio::test]
async fn the_tool_list_is_identical_and_ordered_across_iterations() {
    let result = Scenario::new("list the projects")
        .with_api_route(PROJECTS, one_project(&["us-west-2"]))
        .with_script(vec![
            tool_response(vec![("call-1", "list_projects", json!({}))]),
            text_response("One project."),
        ])
        .run()
        .await;

    // Tools render before the system prompt in the cached prefix, so a reordered
    // or varying tool list invalidates everything after it.
    assert_eq!(result.calls[0].tool_names, result.calls[1].tool_names);
    assert_eq!(result.calls[0].tool_names[0], "list_modules");
}

/// `LlmMessage` has no `PartialEq`; its serialized form is what the client stores,
/// so compare on that.
fn as_json(messages: &[LlmMessage]) -> serde_json::Value {
    serde_json::to_value(messages).expect("messages serialize")
}

/// `(tool_use_id, is_error)` for every tool result the model received on `call`.
fn result_ids_sent_on_call(calls: &[RecordedCall], call: usize) -> Vec<(&str, bool)> {
    let Some(last) = calls[call].messages.last() else {
        return Vec::new();
    };
    last.content
        .iter()
        .filter_map(|c| match c {
            LlmContent::ToolResult {
                tool_use_id,
                is_error,
                ..
            } => Some((tool_use_id.as_str(), *is_error)),
            _ => None,
        })
        .collect()
}

fn event_label(event: &AgentEvent) -> String {
    match event {
        AgentEvent::HistoryAppend { message } => match message.role {
            Role::User => "history_append:user".to_string(),
            Role::Assistant => "history_append:assistant".to_string(),
        },
        AgentEvent::ToolCall { id, .. } => format!("tool_call:{id}"),
        AgentEvent::ToolResult { id, .. } => format!("tool_result:{id}"),
        AgentEvent::Text { .. } => "text".to_string(),
        AgentEvent::Reasoning { .. } => "reasoning".to_string(),
        AgentEvent::Usage { .. } => "usage".to_string(),
        AgentEvent::Truncated { .. } => "truncated".to_string(),
        AgentEvent::Done => "done".to_string(),
        AgentEvent::Error { .. } => "error".to_string(),
    }
}
