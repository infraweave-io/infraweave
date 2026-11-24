//! Builders for scripted model turns, and a reader for what the model was sent.

use infraweave_chat::llm::{LlmContent, LlmResponse, LlmUsage, StopReason};
use serde_json::Value;

use super::fake_llm::RecordedCall;

pub fn tool_call(id: &str, name: &str, input: Value) -> LlmResponse {
    LlmResponse {
        content: vec![LlmContent::ToolUse {
            id: id.to_string(),
            name: name.to_string(),
            input,
        }],
        stop_reason: StopReason::ToolUse,
        usage: LlmUsage::default(),
    }
}

pub fn text_response(text: &str) -> LlmResponse {
    LlmResponse {
        content: vec![LlmContent::Text {
            text: text.to_string(),
        }],
        stop_reason: StopReason::EndTurn,
        usage: LlmUsage::default(),
    }
}

/// The tool result the model received on `converse` call `call`, looked up by
/// `tool_use_id` in that call's final message. Panics if it was not sent, so
/// a result that only reached the emitted history cannot pass for one the
/// model actually saw.
pub fn result_sent_on_call(
    calls: &[RecordedCall],
    call: usize,
    tool_use_id: &str,
) -> (String, bool) {
    calls[call]
        .messages
        .last()
        .into_iter()
        .flat_map(|m| m.content.iter())
        .find_map(|c| match c {
            LlmContent::ToolResult {
                tool_use_id: id,
                content,
                is_error,
            } if id == tool_use_id => Some((content.clone(), *is_error)),
            _ => None,
        })
        .unwrap_or_else(|| panic!("call {call} did not carry a result for `{tool_use_id}`"))
}
