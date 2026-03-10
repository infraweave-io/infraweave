use anyhow::anyhow;
use hcl::{Block, Expression};
use serde::{de::Deserializer, Deserialize, Serialize};
use std::convert::TryFrom;

#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct TfVariable {
    pub name: String,
    #[serde(rename = "type", default = "default_tf_variable_type")]
    pub _type: serde_json::Value,
    #[serde(
        default,
        deserialize_with = "deserialize_default_value_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub default: Option<serde_json::Value>, // Default: missing -> None, explicitly set null in terraform variable -> Some(Value::Null)
    #[serde(default)]
    pub description: String,
    #[serde(default = "default_nullable")]
    pub nullable: bool,
    #[serde(default)]
    pub sensitive: bool,
}

fn default_tf_variable_type() -> serde_json::Value {
    serde_json::Value::String("any".to_string())
}

fn default_nullable() -> bool {
    true
}

// Custom deserializer to treat an explicit JSON null as Some(Value::Null), but missing field as None
fn deserialize_default_value_option<'de, D>(
    deserializer: D,
) -> Result<Option<serde_json::Value>, D::Error>
where
    D: Deserializer<'de>,
{
    let v = serde_json::Value::deserialize(deserializer)?;
    Ok(Some(v))
}

impl Default for TfVariable {
    fn default() -> Self {
        TfVariable {
            name: String::new(),
            _type: serde_json::Value::String("string".to_string()),
            default: None,
            description: String::new(),
            nullable: true,
            sensitive: false,
        }
    }
}

impl TfVariable {
    /// Returns true if this variable is required (i.e. must be provided by the user)
    pub fn required(&self) -> bool {
        if self.default.is_none() {
            return true;
        }

        if !self.nullable && self.default == Some(serde_json::Value::Null) {
            return true;
        }

        false
    }

    pub fn to_block(&self) -> Block {
        Block::builder("variable")
            .add_label(self.name.clone())
            .add_attributes(self.to_attributes())
            .build()
    }

    fn to_attributes(&self) -> Vec<hcl::Attribute> {
        let mut attrs = Vec::new();

        // type
        let type_str = match &self._type {
            serde_json::Value::String(s) => s.clone(),
            other => other.to_string(),
        };
        let edit_expr: hcl::edit::expr::Expression = type_str
            .parse()
            .expect("invalid type expression in TfVariable");
        let type_expr: Expression = edit_expr.into();
        attrs.push(hcl::Attribute::new("type", type_expr));

        // description
        attrs.push(hcl::Attribute::new(
            "description",
            hcl::to_expression(self.description.clone()).expect("invalid description value"),
        ));

        // default (only if present, before nullable/sensitive to keep a stable order)
        if let Some(value) = &self.default {
            let expr =
                hcl::to_expression(value).expect("failed to convert default to HCL expression");
            attrs.push(hcl::Attribute::new("default", expr));
        }

        // nullable
        attrs.push(hcl::Attribute::new(
            "nullable",
            Expression::Bool(self.nullable),
        ));

        // sensitive
        attrs.push(hcl::Attribute::new(
            "sensitive",
            Expression::Bool(self.sensitive),
        ));

        attrs
    }
}

impl TryFrom<&Block> for TfVariable {
    type Error = anyhow::Error;

    fn try_from(block: &Block) -> Result<Self, Self::Error> {
        if block.identifier() != "variable" {
            return Err(anyhow!("Block is not a variable block"));
        }

        if block.labels.len() != 1 {
            return Err(anyhow!(
                "Variable block should have exactly one label, got {:?}",
                block.labels
            ));
        }

        let mut var = TfVariable::default();
        var.name = block.labels[0].as_str().to_string();

        for attr in block.body().attributes() {
            match attr.key() {
                "type" => {
                    var._type = serde_json::Value::String(attr.expr.to_string());
                }
                "default" => {
                    let value: hcl::Value = attr.expr.clone().into();
                    var.default = Some(serde_json::to_value(value)?);
                }
                "description" => {
                    let value: hcl::Value = attr.expr.clone().into();
                    var.description = value.as_str().unwrap_or("").to_string();
                }
                "nullable" => {
                    if let Expression::Bool(b) = attr.expr {
                        var.nullable = b;
                    } else {
                        return Err(anyhow!("nullable must be a bool"));
                    }
                }
                "sensitive" => {
                    if let Expression::Bool(b) = attr.expr {
                        var.sensitive = b;
                    } else {
                        return Err(anyhow!("sensitive must be a bool"));
                    }
                }
                _ => {}
            }
        }

        Ok(var)
    }
}
