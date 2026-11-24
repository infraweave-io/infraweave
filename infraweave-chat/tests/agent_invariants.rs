mod support;

use infraweave_chat::agent::AgentEvent;
use infraweave_chat::llm::{LlmContent, LlmMessage, LlmResponse, LlmUsage, Role, StopReason};
use pretty_assertions::assert_eq;
use serde_json::json;
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
    assert_eq!(sent[0].role, Role::User);
    assert_eq!(sent[1].role, Role::Assistant);
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
