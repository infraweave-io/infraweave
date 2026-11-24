//! The client owns the transcript, so a turn that ended badly can leave it in a shape
//! Bedrock rejects. See `agent::normalize_history`.
//!
//! The shape that genuinely 400s - verified live against `us.anthropic.claude-sonnet-5` -
//! is a trailing assistant turn whose `tool_use` blocks no `tool_result` answers. A
//! trailing tool-result *user* turn (the `MAX_TOOL_ITERATIONS` truncation case) is
//! accepted by Converse even with another user turn appended after it, so it is
//! deliberately left alone; `leaves_a_truncated_turn_intact` pins that decision.

use infraweave_chat::agent::normalize_history;
use infraweave_chat::llm::{LlmContent, LlmMessage, Role};
use serde_json::json;

fn assistant_tool_use(id: &str) -> LlmMessage {
    LlmMessage {
        role: Role::Assistant,
        content: vec![
            LlmContent::Text {
                text: "Let me look that up.".to_string(),
            },
            LlmContent::ToolUse {
                id: id.to_string(),
                name: "list_projects".to_string(),
                input: json!({}),
            },
        ],
    }
}

fn tool_results(ids: &[&str]) -> LlmMessage {
    LlmMessage {
        role: Role::User,
        content: ids
            .iter()
            .map(|id| LlmContent::ToolResult {
                tool_use_id: id.to_string(),
                content: "[]".to_string(),
                is_error: false,
            })
            .collect(),
    }
}

fn assistant_text(text: &str) -> LlmMessage {
    LlmMessage {
        role: Role::Assistant,
        content: vec![LlmContent::Text {
            text: text.to_string(),
        }],
    }
}

fn assert_no_dangling_tool_use(history: &[LlmMessage]) {
    if let Some(last) = history.last() {
        assert!(
            !(last.role == Role::Assistant
                && last
                    .content
                    .iter()
                    .any(|b| matches!(b, LlmContent::ToolUse { .. }))),
            "tail leaves tool_use ids unanswered, which Bedrock rejects: {history:?}"
        );
    }
}

/// Abort / stream-error case: the assistant turn with `tool_use` blocks was appended
/// but the matching tool-result turn never arrived.
#[test]
fn drops_a_trailing_assistant_turn_with_unanswered_tool_use() {
    let history = vec![
        LlmMessage::user_text("what modules are published?"),
        assistant_text("Three modules are published."),
        LlmMessage::user_text("list the projects"),
        assistant_tool_use("call-1"),
    ];

    let normalized = normalize_history(history);

    assert_no_dangling_tool_use(&normalized);
    assert_eq!(
        normalized.len(),
        3,
        "only the aborted assistant turn goes: {normalized:?}"
    );
    assert_eq!(normalized[2].role, Role::User);
}

/// Aborting after a mid-turn tool round trip: the completed `tool_use`/`tool_result`
/// pair before the aborted turn must survive, since it is well-formed.
#[test]
fn keeps_answered_tool_calls_when_dropping_the_aborted_turn() {
    let history = vec![
        LlmMessage::user_text("list the projects"),
        assistant_tool_use("call-1"),
        tool_results(&["call-1"]),
        assistant_tool_use("call-2"),
    ];

    let normalized = normalize_history(history);

    assert_no_dangling_tool_use(&normalized);
    assert_eq!(normalized.len(), 3, "got {normalized:?}");
    assert!(matches!(
        normalized[2].content[0],
        LlmContent::ToolResult { .. }
    ));
}

/// MAX_TOOL_ITERATIONS truncation: the transcript ends with a tool-result user turn.
/// Bedrock accepts this even though the next user turn follows it, so every tool
/// result stays in context.
#[test]
fn leaves_a_truncated_turn_intact() {
    let history = vec![
        LlmMessage::user_text("list the projects"),
        assistant_tool_use("call-1"),
        tool_results(&["call-1"]),
    ];
    let expected = history.len();

    let normalized = normalize_history(history);

    assert_no_dangling_tool_use(&normalized);
    assert_eq!(
        normalized.len(),
        expected,
        "the tool results were already paid for and Converse accepts this tail"
    );
}

/// A well-formed transcript - assistant call, tool results, assistant answer - is
/// left exactly as-is.
#[test]
fn leaves_a_well_formed_transcript_untouched() {
    let history = vec![
        LlmMessage::user_text("list the projects"),
        assistant_tool_use("call-1"),
        tool_results(&["call-1"]),
        assistant_text("There are three projects."),
    ];
    let expected = history.len();

    let normalized = normalize_history(history);

    assert_no_dangling_tool_use(&normalized);
    assert_eq!(normalized.len(), expected);
}

/// An empty history (the first turn of a conversation) is untouched.
#[test]
fn empty_history_is_untouched() {
    assert!(normalize_history(vec![]).is_empty());
}
