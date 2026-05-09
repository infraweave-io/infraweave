//! Bedrock Converse implementation of `LlmClient`.

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use aws_sdk_bedrockruntime::{
    types::{
        ContentBlock, ConversationRole, Message, StopReason as BedrockStopReason,
        SystemContentBlock, Tool as BedrockTool, ToolConfiguration, ToolInputSchema,
        ToolResultBlock, ToolResultContentBlock, ToolResultStatus, ToolSpecification, ToolUseBlock,
    },
    Client,
};
use aws_smithy_types::Document;
use serde_json::{Map, Value};

use super::{LlmClient, LlmContent, LlmMessage, LlmResponse, Role, StopReason, ToolDef};

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
        let mut req = self
            .client
            .converse()
            .model_id(&self.model_id)
            .system(SystemContentBlock::Text(system.to_string()));

        for m in messages {
            req = req.messages(to_bedrock_message(m)?);
        }

        if !tools.is_empty() {
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
            req = req.tool_config(
                tool_config
                    .build()
                    .map_err(|e| anyhow!("invalid tool config: {e}"))?,
            );
        }

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
                _ => {
                    // Ignore reasoning/guardrail/etc. blocks for now.
                }
            }
        }

        Ok(LlmResponse {
            content,
            stop_reason,
        })
    }
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
