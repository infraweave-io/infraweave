use anyhow::anyhow;
use hcl::Expression;
use serde::{Deserialize, Serialize};

#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
pub struct TfOutput {
    pub name: String,
    pub value: String,
    pub description: String,
    pub sensitive: Option<bool>,
}

impl TfOutput {
    pub fn from_block(block: &hcl::Block) -> Result<Self, anyhow::Error> {
        if block.identifier() != "output" {
            return Err(anyhow!("Block is not an output block"));
        }
        if block.labels.len() > 1 {
            return Err(anyhow!(
                "Output block should have a single label! {:?}",
                block.labels
            ));
        }
        let name = block.labels[0].as_str().to_string();
        let description = block
            .body()
            .attributes()
            .find(|attr| attr.key() == "description")
            .map_or_else(
                || "".to_string(),
                |attr| {
                    hcl::Value::from(attr.expr.clone())
                        .as_str()
                        .unwrap()
                        .to_string()
                },
            );
        let value = block
            .body()
            .attributes()
            .find(|attr| attr.key() == "value")
            .map_or_else(|| "".to_string(), |attr| attr.expr.to_string());
        let sensitive = block
            .body()
            .attributes()
            .find(|attr| attr.key() == "sensitive")
            .map_or_else(
                || None,
                |attr| match attr.expr {
                    Expression::Bool(val) => Some(val),
                    _ => None,
                },
            );
        Ok(TfOutput {
            name,
            value,
            description,
            sensitive,
        })
    }

    pub fn to_block(&self) -> hcl::Block {
        hcl::Block::builder("output")
            .add_label(self.name.clone())
            .add_attributes(self.to_attributes())
            .build()
    }

    fn to_attributes(&self) -> Vec<hcl::Attribute> {
        let value: hcl::edit::expr::Expression = self.value.parse().unwrap();
        let value: hcl::Expression = value.into();
        let mut attrs = vec![
            hcl::Attribute::new(
                "description",
                hcl::to_expression(self.description.clone()).unwrap(),
            ),
            hcl::Attribute::new("value", value),
        ];
        if let Some(val) = self.sensitive {
            attrs.push(hcl::Attribute::new("sensitive", Expression::Bool(val)));
        }
        attrs
    }
}

#[cfg(test)]
mod tests {

    #[cfg(test)]
    mod from_block {

        use crate::TfOutput;

        #[test]
        fn wrong_block() {
            assert!(TfOutput::from_block(
                &hcl::parse(
                    r#"
                        variable "input" {
                            type = string
                        }
                        "#
                )
                .unwrap()
                .blocks()
                .next()
                .unwrap()
            )
            .is_err())
        }

        #[test]
        fn no_description() {
            assert_eq!(
                TfOutput::from_block(
                    &hcl::parse(
                        r#"
                        output "result" {
                            value = module.some.field
                        }
                        "#
                    )
                    .unwrap()
                    .blocks()
                    .next()
                    .unwrap()
                )
                .unwrap(),
                TfOutput {
                    name: "result".to_string(),
                    description: "".to_string(),
                    value: "module.some.field".to_string(),
                    sensitive: None,
                }
            )
        }

        #[test]
        fn with_description() {
            assert_eq!(
                TfOutput::from_block(
                    &hcl::parse(
                        r#"
                        output "result" {
                            description = "result of execution"
                            value = module.some.field
                        }
                        "#
                    )
                    .unwrap()
                    .blocks()
                    .next()
                    .unwrap()
                )
                .unwrap(),
                TfOutput {
                    name: "result".to_string(),
                    description: "result of execution".to_string(),
                    value: "module.some.field".to_string(),
                    sensitive: None,
                }
            )
        }

        #[test]
        fn is_not_sensitive() {
            assert_eq!(
                TfOutput::from_block(
                    &hcl::parse(
                        r#"
                        output "result" {
                            description = "result of execution"
                            value = module.some.field
                            sensitive = false
                        }
                        "#
                    )
                    .unwrap()
                    .blocks()
                    .next()
                    .unwrap()
                )
                .unwrap(),
                TfOutput {
                    name: "result".to_string(),
                    description: "result of execution".to_string(),
                    value: "module.some.field".to_string(),
                    sensitive: Some(false),
                }
            )
        }

        #[test]
        fn is_sensitive() {
            assert_eq!(
                TfOutput::from_block(
                    &hcl::parse(
                        r#"
                        output "result" {
                            description = "result of execution"
                            value = module.some.field
                            sensitive = true
                        }
                        "#
                    )
                    .unwrap()
                    .blocks()
                    .next()
                    .unwrap()
                )
                .unwrap(),
                TfOutput {
                    name: "result".to_string(),
                    description: "result of execution".to_string(),
                    value: "module.some.field".to_string(),
                    sensitive: Some(true),
                }
            )
        }

        #[test]
        fn multiple_labels_not_allowed() {
            assert!(TfOutput::from_block(
                &hcl::parse(
                    r#"
                        output "one" "two" {
                            description = "result of execution"
                            value = module.some.field
                            sensitive = true
                        }
                        "#
                )
                .unwrap()
                .blocks()
                .next()
                .unwrap()
            )
            .is_err())
        }
    }

    #[cfg(test)]
    mod to_block {
        use crate::TfOutput;

        #[test]
        fn no_description() {
            assert_eq!(
                TfOutput {
                    name: "result".to_string(),
                    description: "".to_string(),
                    value: "module.some.field".to_string(),
                    sensitive: None
                }
                .to_block(),
                hcl::parse(
                    r#"
                    output "result" {
                        description = ""
                        value = module.some.field
                    }
                    "#
                )
                .unwrap()
                .blocks()
                .next()
                .unwrap()
                .clone()
            )
        }

        #[test]
        fn with_description() {
            assert_eq!(
                TfOutput {
                    name: "result".to_string(),
                    description: "result of execution".to_string(),
                    value: "module.some.field".to_string(),
                    sensitive: None,
                }
                .to_block(),
                hcl::parse(
                    r#"
                    output "result" {
                        description = "result of execution"
                        value = module.some.field
                    }
                    "#
                )
                .unwrap()
                .blocks()
                .next()
                .unwrap()
                .clone()
            )
        }

        #[test]
        fn is_not_sensitive() {
            assert_eq!(
                TfOutput {
                    name: "result".to_string(),
                    description: "result of execution".to_string(),
                    value: "module.some.field".to_string(),
                    sensitive: Some(false),
                }
                .to_block(),
                hcl::parse(
                    r#"
                    output "result" {
                        description = "result of execution"
                        value = module.some.field
                        sensitive = false
                    }
                    "#
                )
                .unwrap()
                .blocks()
                .next()
                .unwrap()
                .clone()
            )
        }

        #[test]
        fn is_sensitive() {
            assert_eq!(
                TfOutput {
                    name: "result".to_string(),
                    description: "result of execution".to_string(),
                    value: "module.some.field".to_string(),
                    sensitive: Some(true),
                }
                .to_block(),
                hcl::parse(
                    r#"
                    output "result" {
                        description = "result of execution"
                        value = module.some.field
                        sensitive = true
                    }
                    "#
                )
                .unwrap()
                .blocks()
                .next()
                .unwrap()
                .clone()
            )
        }
    }
}
