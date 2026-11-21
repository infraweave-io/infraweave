//! Bedrock Converse implementation of `LlmClient`.

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use aws_sdk_bedrockruntime::{
    types::{
        CachePointBlock, CachePointType, ContentBlock, ContentBlockDelta, ContentBlockStart,
        ConversationRole, ConverseStreamOutput, Message, ReasoningContentBlock,
        ReasoningContentBlockDelta, ReasoningTextBlock, StopReason as BedrockStopReason,
        SystemContentBlock, Tool as BedrockTool, ToolConfiguration, ToolInputSchema,
        ToolResultBlock, ToolResultContentBlock, ToolResultStatus, ToolSpecification, ToolUseBlock,
    },
    Client,
};
use aws_smithy_types::Blob;
use aws_smithy_types::Document;
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use serde_json::{Map, Value};
use tokio::sync::mpsc;

use super::{
    stream_assembly::StreamAssembler, LlmClient, LlmContent, LlmDelta, LlmMessage, LlmResponse,
    LlmUsage, Role, StopReason, ToolDef,
};

pub struct BedrockClient {
    client: Client,
    model_id: String,
}

impl BedrockClient {
    pub async fn from_env(model_id: impl Into<String>) -> Self {
        let config = aws_config::load_from_env().await;
        Self {
            client: Client::new(&config),
            model_id: model_id.into(),
        }
    }
}

#[async_trait]
impl LlmClient for BedrockClient {
    async fn converse(
        &self,
        system: &str,
        messages: &[LlmMessage],
        tools: &[ToolDef],
    ) -> Result<LlmResponse> {
        // Bedrock renders `tools -> system -> messages` and caching is a prefix
        // match, so a cache point after the tool list and another after the
        // system prompt cover everything that is stable across a conversation.
        let system_cache_point = CachePointBlock::builder()
            .r#type(CachePointType::Default)
            .build()
            .map_err(|e| anyhow!("invalid system cache point: {e}"))?;

        let mut req = self
            .client
            .converse()
            .model_id(&self.model_id)
            .system(SystemContentBlock::Text(system.to_string()))
            .system(SystemContentBlock::CachePoint(system_cache_point));

        for m in messages {
            req = req.messages(to_bedrock_message(m)?);
        }

        if !tools.is_empty() {
            req = req.tool_config(build_tool_config(tools)?);
        }

        let thinking = Document::Object(std::collections::HashMap::from([(
            "thinking".to_string(),
            Document::Object(std::collections::HashMap::from([
                ("type".to_string(), Document::String("adaptive".to_string())),
                (
                    "display".to_string(),
                    Document::String("summarized".to_string()),
                ),
            ])),
        )]));
        req = req.additional_model_request_fields(thinking);

        let resp = req.send().await.map_err(|e| {
            use std::error::Error as _;
            let mut detail = format!("{e}");
            let mut source: Option<&dyn std::error::Error> = e.source();
            while let Some(s) = source {
                detail.push_str(" | ");
                detail.push_str(&s.to_string());
                source = s.source();
            }
            anyhow!("Bedrock Converse failed: {detail}")
        })?;

        let stop_reason = match resp.stop_reason() {
            BedrockStopReason::EndTurn => StopReason::EndTurn,
            BedrockStopReason::ToolUse => StopReason::ToolUse,
            BedrockStopReason::MaxTokens => StopReason::MaxTokens,
            other => StopReason::Other(format!("{other:?}")),
        };

        let output = resp
            .output()
            .ok_or_else(|| anyhow!("Bedrock response has no output"))?;
        let msg = output
            .as_message()
            .map_err(|_| anyhow!("Bedrock output was not a message"))?;

        let mut content = Vec::with_capacity(msg.content().len());
        for block in msg.content() {
            match block {
                ContentBlock::Text(t) => content.push(LlmContent::Text { text: t.clone() }),
                ContentBlock::ToolUse(tu) => content.push(LlmContent::ToolUse {
                    id: tu.tool_use_id().to_string(),
                    name: tu.name().to_string(),
                    input: document_to_value(tu.input()),
                }),
                ContentBlock::ReasoningContent(rc) => match rc {
                    ReasoningContentBlock::ReasoningText(rt) => {
                        content.push(LlmContent::Reasoning {
                            text: rt.text().to_string(),
                            signature: rt.signature().map(|s| s.to_string()),
                        })
                    }
                    ReasoningContentBlock::RedactedContent(blob) => {
                        content.push(LlmContent::RedactedReasoning {
                            data: BASE64.encode(blob.as_ref()),
                        })
                    }
                    // Smithy unions are non_exhaustive; unknown reasoning shapes are dropped
                    // rather than failing the turn.
                    _ => {}
                },
                _ => {
                    // Guardrail, citation and other blocks are not surfaced yet.
                }
            }
        }

        let usage = resp
            .usage()
            .map(|u| LlmUsage {
                input_tokens: u.input_tokens(),
                output_tokens: u.output_tokens(),
                cache_read_input_tokens: u.cache_read_input_tokens().unwrap_or(0),
                cache_write_input_tokens: u.cache_write_input_tokens().unwrap_or(0),
            })
            .unwrap_or_default();

        tracing::debug!(
            input_tokens = usage.input_tokens,
            output_tokens = usage.output_tokens,
            cache_read = usage.cache_read_input_tokens,
            cache_write = usage.cache_write_input_tokens,
            "bedrock converse usage"
        );

        Ok(LlmResponse {
            content,
            stop_reason,
            usage,
        })
    }

    async fn converse_stream(
        &self,
        system: &str,
        messages: &[LlmMessage],
        tools: &[ToolDef],
        tx: &mpsc::Sender<LlmDelta>,
    ) -> Result<LlmResponse> {
        let mut req = self
            .client
            .converse_stream()
            .model_id(&self.model_id)
            .system(SystemContentBlock::Text(system.to_string()))
            .system(SystemContentBlock::CachePoint(
                CachePointBlock::builder()
                    .r#type(CachePointType::Default)
                    .build()
                    .map_err(|e| anyhow!("invalid system cache point: {e}"))?,
            ));

        for m in messages {
            req = req.messages(to_bedrock_message(m)?);
        }
        if !tools.is_empty() {
            req = req.tool_config(build_tool_config(tools)?);
        }

        let mut resp = req.send().await.map_err(|e| {
            use std::error::Error as _;
            let mut detail = format!("{e}");
            let mut source: Option<&dyn std::error::Error> = e.source();
            while let Some(s) = source {
                detail.push_str(" | ");
                detail.push_str(&s.to_string());
                source = s.source();
            }
            anyhow!("Bedrock ConverseStream failed: {detail}")
        })?;

        let mut assembler = StreamAssembler::new();
        let mut stop_reason = StopReason::EndTurn;
        let mut usage = LlmUsage::default();

        while let Some(event) = resp
            .stream
            .recv()
            .await
            .map_err(|e| anyhow!("Bedrock stream error: {e}"))?
        {
            match event {
                ConverseStreamOutput::ContentBlockStart(ev) => {
                    if let Some(ContentBlockStart::ToolUse(tu)) = ev.start() {
                        assembler.start_tool_use(
                            ev.content_block_index(),
                            tu.tool_use_id().to_string(),
                            tu.name().to_string(),
                        );
                    }
                }
                ConverseStreamOutput::ContentBlockDelta(ev) => {
                    let index = ev.content_block_index();
                    match ev.delta() {
                        Some(ContentBlockDelta::Text(t)) => {
                            assembler.push_text(index, t);
                            let _ = tx.send(LlmDelta::Text { text: t.clone() }).await;
                        }
                        Some(ContentBlockDelta::ToolUse(tu)) => {
                            assembler.push_tool_input(index, tu.input());
                        }
                        Some(ContentBlockDelta::ReasoningContent(rc)) => match rc {
                            ReasoningContentBlockDelta::Text(t) => {
                                assembler.push_reasoning_text(index, t);
                                let _ = tx.send(LlmDelta::Reasoning { text: t.clone() }).await;
                            }
                            ReasoningContentBlockDelta::Signature(sig) => {
                                assembler.set_reasoning_signature(index, sig);
                            }
                            _ => {}
                        },
                        _ => {}
                    }
                }
                ConverseStreamOutput::MessageStop(ev) => {
                    stop_reason = match ev.stop_reason() {
                        BedrockStopReason::EndTurn => StopReason::EndTurn,
                        BedrockStopReason::ToolUse => StopReason::ToolUse,
                        BedrockStopReason::MaxTokens => StopReason::MaxTokens,
                        other => StopReason::Other(format!("{other:?}")),
                    };
                }
                ConverseStreamOutput::Metadata(ev) => {
                    if let Some(u) = ev.usage() {
                        usage = LlmUsage {
                            input_tokens: u.input_tokens(),
                            output_tokens: u.output_tokens(),
                            cache_read_input_tokens: u.cache_read_input_tokens().unwrap_or(0),
                            cache_write_input_tokens: u.cache_write_input_tokens().unwrap_or(0),
                        };
                    }
                }
                _ => {}
            }
        }

        Ok(LlmResponse {
            content: assembler.finish()?,
            stop_reason,
            usage,
        })
    }
}

fn build_tool_config(tools: &[ToolDef]) -> Result<ToolConfiguration> {
    let mut tool_config = ToolConfiguration::builder();
    for t in tools {
        let spec = ToolSpecification::builder()
            .name(t.name.clone())
            .description(t.description.clone())
            .input_schema(ToolInputSchema::Json(value_to_document(&t.input_schema)))
            .build()
            .map_err(|e| anyhow!("invalid tool spec for `{}`: {e}", t.name))?;
        tool_config = tool_config.tools(BedrockTool::ToolSpec(spec));
    }
    tool_config = tool_config.tools(BedrockTool::CachePoint(
        CachePointBlock::builder()
            .r#type(CachePointType::Default)
            .build()
            .map_err(|e| anyhow!("invalid tool cache point: {e}"))?,
    ));
    tool_config
        .build()
        .map_err(|e| anyhow!("invalid tool config: {e}"))
}

fn to_bedrock_message(m: &LlmMessage) -> Result<Message> {
    let role = match m.role {
        Role::User => ConversationRole::User,
        Role::Assistant => ConversationRole::Assistant,
    };
    let mut builder = Message::builder().role(role);
    for block in &m.content {
        let cb = match block {
            LlmContent::Text { text } => ContentBlock::Text(text.clone()),
            LlmContent::Reasoning { text, signature } => {
                let mut rb = ReasoningTextBlock::builder().text(text);
                if let Some(sig) = signature {
                    rb = rb.signature(sig);
                }
                ContentBlock::ReasoningContent(ReasoningContentBlock::ReasoningText(
                    rb.build()
                        .map_err(|e| anyhow!("invalid reasoning block: {e}"))?,
                ))
            }
            LlmContent::RedactedReasoning { data } => {
                let bytes = BASE64
                    .decode(data)
                    .map_err(|e| anyhow!("invalid base64 in redacted reasoning: {e}"))?;
                ContentBlock::ReasoningContent(ReasoningContentBlock::RedactedContent(Blob::new(
                    bytes,
                )))
            }
            LlmContent::ToolUse { id, name, input } => ContentBlock::ToolUse(
                ToolUseBlock::builder()
                    .tool_use_id(id)
                    .name(name)
                    .input(value_to_document(input))
                    .build()
                    .map_err(|e| anyhow!("invalid tool_use block: {e}"))?,
            ),
            LlmContent::ToolResult {
                tool_use_id,
                content,
                is_error,
            } => ContentBlock::ToolResult(
                ToolResultBlock::builder()
                    .tool_use_id(tool_use_id)
                    .content(ToolResultContentBlock::Text(content.clone()))
                    .status(if *is_error {
                        ToolResultStatus::Error
                    } else {
                        ToolResultStatus::Success
                    })
                    .build()
                    .map_err(|e| anyhow!("invalid tool_result block: {e}"))?,
            ),
        };
        builder = builder.content(cb);
    }
    builder
        .build()
        .map_err(|e| anyhow!("invalid Bedrock message: {e}"))
}

fn value_to_document(v: &Value) -> Document {
    match v {
        Value::Null => Document::Null,
        Value::Bool(b) => Document::Bool(*b),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Document::Number(aws_smithy_types::Number::NegInt(i))
            } else if let Some(u) = n.as_u64() {
                Document::Number(aws_smithy_types::Number::PosInt(u))
            } else {
                Document::Number(aws_smithy_types::Number::Float(n.as_f64().unwrap_or(0.0)))
            }
        }
        Value::String(s) => Document::String(s.clone()),
        Value::Array(a) => Document::Array(a.iter().map(value_to_document).collect()),
        Value::Object(o) => {
            let mut map = std::collections::HashMap::with_capacity(o.len());
            for (k, v) in o {
                map.insert(k.clone(), value_to_document(v));
            }
            Document::Object(map)
        }
    }
}

fn document_to_value(d: &Document) -> Value {
    match d {
        Document::Null => Value::Null,
        Document::Bool(b) => Value::Bool(*b),
        Document::Number(n) => match n {
            aws_smithy_types::Number::PosInt(u) => Value::Number((*u).into()),
            aws_smithy_types::Number::NegInt(i) => Value::Number((*i).into()),
            aws_smithy_types::Number::Float(f) => serde_json::Number::from_f64(*f)
                .map(Value::Number)
                .unwrap_or(Value::Null),
        },
        Document::String(s) => Value::String(s.clone()),
        Document::Array(a) => Value::Array(a.iter().map(document_to_value).collect()),
        Document::Object(o) => {
            let mut map = Map::with_capacity(o.len());
            for (k, v) in o {
                map.insert(k.clone(), document_to_value(v));
            }
            Value::Object(map)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::{LlmContent, LlmMessage, Role};

    #[test]
    fn reasoning_blocks_round_trip_through_bedrock_types() {
        let original = LlmMessage {
            role: Role::Assistant,
            content: vec![
                LlmContent::Reasoning {
                    text: "The deployment failed at the S3 step.".to_string(),
                    signature: Some("sig-abc123".to_string()),
                },
                LlmContent::Text {
                    text: "The bucket already exists.".to_string(),
                },
            ],
        };

        let bedrock = to_bedrock_message(&original).expect("conversion should succeed");
        let blocks = bedrock.content();
        assert_eq!(blocks.len(), 2);

        let ContentBlock::ReasoningContent(ReasoningContentBlock::ReasoningText(rt)) = &blocks[0]
        else {
            panic!("first block should be reasoning text, got {:?}", blocks[0]);
        };
        assert_eq!(rt.text(), "The deployment failed at the S3 step.");
        assert_eq!(rt.signature(), Some("sig-abc123"));
    }

    #[test]
    fn redacted_reasoning_decodes_from_base64() {
        let original = LlmMessage {
            role: Role::Assistant,
            content: vec![LlmContent::RedactedReasoning {
                data: "aGVsbG8=".to_string(), // "hello"
            }],
        };

        let bedrock = to_bedrock_message(&original).expect("conversion should succeed");
        let ContentBlock::ReasoningContent(ReasoningContentBlock::RedactedContent(blob)) =
            &bedrock.content()[0]
        else {
            panic!("expected redacted reasoning content");
        };
        assert_eq!(blob.as_ref(), b"hello");
    }
}
