//! Accumulates Bedrock `converse_stream` events into finished content blocks.
//!
//! Split out from the Bedrock client so the fiddly part - partial tool-input
//! JSON arriving as string fragments - is unit-testable without AWS.

use anyhow::{anyhow, Result};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use std::collections::BTreeMap;

use crate::llm::LlmContent;

#[derive(Debug, Default)]
enum Partial {
    #[default]
    Unknown,
    Text(String),
    Reasoning {
        text: String,
        signature: Option<String>,
    },
    /// Opaque encrypted reasoning bytes. Accumulated raw and base64-encoded on
    /// `finish` so the block can be echoed back verbatim next turn.
    Redacted(Vec<u8>),
    ToolUse {
        id: String,
        name: String,
        input_json: String,
    },
}

/// Accumulates blocks by `content_block_index`, then emits them in index order.
#[derive(Debug, Default)]
pub struct StreamAssembler {
    blocks: BTreeMap<i32, Partial>,
}

impl StreamAssembler {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn start_text(&mut self, index: i32) {
        self.blocks.insert(index, Partial::Text(String::new()));
    }

    pub fn start_reasoning(&mut self, index: i32) {
        self.blocks.insert(
            index,
            Partial::Reasoning {
                text: String::new(),
                signature: None,
            },
        );
    }

    pub fn start_tool_use(&mut self, index: i32, id: String, name: String) {
        self.blocks.insert(
            index,
            Partial::ToolUse {
                id,
                name,
                input_json: String::new(),
            },
        );
    }

    pub fn push_text(&mut self, index: i32, chunk: &str) {
        match self.blocks.entry(index).or_default() {
            Partial::Text(s) => s.push_str(chunk),
            slot => *slot = Partial::Text(chunk.to_string()),
        }
    }

    pub fn push_reasoning_text(&mut self, index: i32, chunk: &str) {
        match self.blocks.entry(index).or_default() {
            Partial::Reasoning { text, .. } => text.push_str(chunk),
            slot => {
                *slot = Partial::Reasoning {
                    text: chunk.to_string(),
                    signature: None,
                }
            }
        }
    }

    /// Sonnet 5 can return a reasoning block with empty `text` and only a
    /// `signature`. In the streaming path that arrives as a
    /// `Signature` delta with no preceding `Text` delta, so this must create the
    /// slot - like `push_reasoning_text` does - rather than only updating an
    /// existing one, or the block is silently lost from the assistant turn.
    pub fn set_reasoning_signature(&mut self, index: i32, sig: &str) {
        match self.blocks.entry(index).or_default() {
            Partial::Reasoning { signature, .. } => *signature = Some(sig.to_string()),
            slot => {
                *slot = Partial::Reasoning {
                    text: String::new(),
                    signature: Some(sig.to_string()),
                }
            }
        }
    }

    /// Accumulate opaque redacted-reasoning bytes for a block.
    pub fn push_redacted(&mut self, index: i32, chunk: &[u8]) {
        match self.blocks.entry(index).or_default() {
            Partial::Redacted(bytes) => bytes.extend_from_slice(chunk),
            slot => *slot = Partial::Redacted(chunk.to_vec()),
        }
    }

    pub fn push_tool_input(&mut self, index: i32, chunk: &str) {
        if let Some(Partial::ToolUse { input_json, .. }) = self.blocks.get_mut(&index) {
            input_json.push_str(chunk);
        }
    }

    /// Consume the accumulator and produce finished blocks in index order.
    pub fn finish(self) -> Result<Vec<LlmContent>> {
        let mut out = Vec::with_capacity(self.blocks.len());
        for (index, partial) in self.blocks {
            match partial {
                Partial::Unknown => {}
                Partial::Text(text) => out.push(LlmContent::Text { text }),
                Partial::Reasoning { text, signature } => {
                    out.push(LlmContent::Reasoning { text, signature })
                }
                Partial::Redacted(bytes) => out.push(LlmContent::RedactedReasoning {
                    data: BASE64.encode(&bytes),
                }),
                Partial::ToolUse {
                    id,
                    name,
                    input_json,
                } => {
                    // An empty-argument tool call streams no input fragments at all.
                    let input = if input_json.trim().is_empty() {
                        serde_json::Value::Object(serde_json::Map::new())
                    } else {
                        serde_json::from_str(&input_json).map_err(|e| {
                            anyhow!(
                                "tool `{name}` (block {index}) streamed invalid input json: {e}"
                            )
                        })?
                    };
                    out.push(LlmContent::ToolUse { id, name, input });
                }
            }
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn reassembles_tool_input_split_across_fragments() {
        let mut a = StreamAssembler::new();
        a.start_tool_use(0, "call-1".to_string(), "list_deployments".to_string());
        // Bedrock splits the input JSON at arbitrary byte boundaries.
        a.push_tool_input(0, "{\"project_id\":\"1234");
        a.push_tool_input(0, "56789012\",\"region\":");
        a.push_tool_input(0, "\"us-west-2\"}");

        let blocks = a.finish().expect("assembly should succeed");
        assert_eq!(blocks.len(), 1);
        let LlmContent::ToolUse { id, name, input } = &blocks[0] else {
            panic!("expected a tool use block");
        };
        assert_eq!(id, "call-1");
        assert_eq!(name, "list_deployments");
        assert_eq!(
            input,
            &json!({"project_id": "123456789012", "region": "us-west-2"})
        );
    }

    #[test]
    fn empty_tool_input_becomes_an_empty_object() {
        let mut a = StreamAssembler::new();
        a.start_tool_use(0, "call-1".to_string(), "list_projects".to_string());

        let blocks = a.finish().expect("assembly should succeed");
        let LlmContent::ToolUse { input, .. } = &blocks[0] else {
            panic!("expected a tool use block");
        };
        assert_eq!(input, &json!({}));
    }

    #[test]
    fn preserves_block_order_by_index() {
        let mut a = StreamAssembler::new();
        a.start_text(1);
        a.push_text(1, "second");
        a.start_reasoning(0);
        a.push_reasoning_text(0, "first");
        a.set_reasoning_signature(0, "sig-1");

        let blocks = a.finish().expect("assembly should succeed");
        assert!(matches!(blocks[0], LlmContent::Reasoning { .. }));
        assert!(matches!(blocks[1], LlmContent::Text { .. }));
    }

    #[test]
    fn signature_only_reasoning_block_is_preserved() {
        // Sonnet 5 can emit a reasoning block that carries only a signature: the
        // stream then has a Signature delta with no preceding Text delta.
        let mut a = StreamAssembler::new();
        a.set_reasoning_signature(0, "sig-only");
        a.push_text(1, "the answer");

        let blocks = a.finish().expect("assembly should succeed");
        assert_eq!(blocks.len(), 2, "the reasoning block must not be dropped");
        let LlmContent::Reasoning { text, signature } = &blocks[0] else {
            panic!("expected a reasoning block, got {:?}", blocks[0]);
        };
        assert_eq!(text, "");
        assert_eq!(signature.as_deref(), Some("sig-only"));
    }

    #[test]
    fn redacted_reasoning_delta_is_assembled() {
        let mut a = StreamAssembler::new();
        // Bedrock may split the opaque payload across deltas like any other block.
        a.push_redacted(0, b"hel");
        a.push_redacted(0, b"lo");
        a.push_text(1, "done");

        let blocks = a.finish().expect("assembly should succeed");
        assert_eq!(blocks.len(), 2);
        let LlmContent::RedactedReasoning { data } = &blocks[0] else {
            panic!("expected redacted reasoning, got {:?}", blocks[0]);
        };
        assert_eq!(data, "aGVsbG8="); // base64("hello")
    }

    #[test]
    fn malformed_tool_input_is_an_error_not_a_panic() {
        let mut a = StreamAssembler::new();
        a.start_tool_use(0, "call-1".to_string(), "list_modules".to_string());
        a.push_tool_input(0, "{\"broken\":");

        let err = a.finish().expect_err("truncated json should error");
        assert!(err.to_string().contains("invalid input json"));
    }
}
