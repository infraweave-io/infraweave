use env_defs::TfOutput;
use env_defs::TfRequiredProvider;
use env_defs::TfVariable;
use hcl::de;
use hcl::Block;
use hcl::Expression;
use hcl::ObjectKey;
use log::debug;
use std::collections::HashMap;
use std::io::{self, ErrorKind};

#[allow(dead_code)]
pub fn validate_tf_backend_not_set(contents: &str) -> Result<(), String> {
    let parsed_hcl: HashMap<String, serde_json::Value> =
        de::from_str(contents).map_err(|err| format!("Failed to parse HCL: {}", err))?;

    if let Some(terraform_blocks) = parsed_hcl.get("terraform") {
        if terraform_blocks.is_object() {
            ensure_no_backend_block(terraform_blocks).unwrap();
        } else if terraform_blocks.is_array() {
            for terraform_block in terraform_blocks.as_array().unwrap() {
                ensure_no_backend_block(terraform_block).unwrap();
            }
        } else {
            return Ok(());
        }
    }

    Ok(())
}

fn ensure_no_backend_block(terraform_block: &serde_json::Value) -> Result<(), String> {
    // Check if the backend block is present in the terraform configuration
    if let Some(_backend_blocks) = terraform_block.get("backend") {
        panic!(
            "Backend block was found in the terraform backend configuration\n{}",
            get_block_help(terraform_block)
        );
    }
    Ok(())
}

pub fn get_block_help(block: &serde_json::Value) -> String {
    let help = format!(
        r#"
Please make sure you do not set any backend block in your terraform code, this is handled by the platform.

Remove this block from your terraform configuration to proceed:

{}
    "#,
        hcl::to_string(block).unwrap()
    );
    help.to_string()
}

#[allow(dead_code)]
pub fn get_variables_from_tf_files(contents: &str) -> Result<Vec<TfVariable>, String> {
    let parsed_hcl: HashMap<String, serde_json::Value> =
        de::from_str(contents).map_err(|err| format!("Failed to parse HCL: {}", err))?;

    let mut variables = Vec::new();

    // Iterate through the HCL blocks (assuming `parsed_hcl` is correctly structured)
    if let Some(var_blocks) = parsed_hcl.get("variable") {
        if let Some(var_map) = var_blocks.as_object() {
            for (var_name, var_attrs) in var_map {
                // Extract the attributes for the variable (type, default, description, etc.)
                let variable_type = var_attrs
                    .get("type")
                    .cloned()
                    .unwrap_or(serde_json::Value::String("string".to_string()));
                // Handle type values that might be wrapped in ${}
                let variable_type = match variable_type {
                    serde_json::Value::String(s) => {
                        // Strip ${} if present
                        if s.starts_with("${") && s.ends_with("}") {
                            serde_json::Value::String(
                                s.trim_start_matches("${").trim_end_matches("}").to_string(),
                            )
                        } else {
                            serde_json::Value::String(s)
                        }
                    }
                    _ => variable_type, // Keep as is for complex types like maps
                };
                let default_value = var_attrs
                    .get("default")
                    .cloned()
                    .unwrap_or(serde_json::Value::Null);
                let description = var_attrs
                    .get("description")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let nullable = var_attrs
                    .get("nullable")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(true);
                let sensitive = var_attrs
                    .get("sensitive")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);

                let variable = TfVariable {
                    name: var_name.clone(),
                    _type: variable_type,
                    default: default_value,
                    description: description,
                    nullable: nullable,
                    sensitive: sensitive,
                };

                debug!("Parsing variable block {:?} as {:?}", var_attrs, variable);
                variables.push(variable);
            }
        }
    }

    Ok(variables)
}

#[allow(dead_code)]
pub fn get_outputs_from_tf_files(contents: &str) -> Result<Vec<env_defs::TfOutput>, String> {
    let hcl_body = hcl::parse(contents)
        .map_err(|_| io::Error::new(ErrorKind::InvalidData, "Failed to parse HCL content"))
        .unwrap();

    let mut outputs = Vec::new();

    for block in hcl_body.blocks() {
        if block.identifier() == "output" {
            // Exclude outputs that are not meant to be exported, such as "value"
            let attrs = get_attributes(block, vec!["value".to_string()]);

            if block.labels().len() != 1 {
                panic!(
                    "Expected exactly one label for output block, found: {:?}",
                    block.labels()
                );
            }
            let output_name = block.labels().first().unwrap().as_str().to_string();

            let output = TfOutput {
                name: output_name,
                description: attrs
                    .get("description")
                    .unwrap_or(&"".to_string())
                    .to_string(),
                value: attrs.get("value").unwrap_or(&"".to_string()).to_string(),
            };

            debug!("Parsing output block {:?} as {:?}", block, output);
            outputs.push(output);
        }
    }
    // log::info!("variables: {:?}", serde_json::to_string(&variables));
    Ok(outputs)
}

#[allow(dead_code)]
pub fn get_tf_required_providers_from_tf_files(
    contents: &str,
) -> Result<Vec<env_defs::TfRequiredProvider>, String> {
    let hcl_body = hcl::parse(contents)
        .map_err(|_| io::Error::new(ErrorKind::InvalidData, "Failed to parse HCL content"))
        .unwrap();

    let mut required_providers = Vec::new();

    for block in hcl_body.blocks() {
        if block.identifier() == "terraform" {
            for inside_block in block.body().blocks() {
                if inside_block.identifier() == "required_providers" {
                    let body = inside_block.body();
                    for attribute in body.attributes() {
                        let required_provider_name = attribute.key().to_string();
                        let attrs: HashMap<String, String> =
                            split_expr(attribute.expr(), &attribute.key())
                                .iter()
                                .map(|(k, v)| (k.clone(), v.clone()))
                                .collect();

                        let required_provider = TfRequiredProvider {
                            name: required_provider_name.clone(),
                            source: attrs
                                .get("source")
                                .expect(&format!(
                                    "source is missing in {} in required_providers",
                                    required_provider_name
                                ))
                                .to_string(),
                            version: attrs
                                .get("version")
                                .expect(&format!(
                                    "version is missing in {} in required_providers",
                                    required_provider_name
                                ))
                                .to_string(),
                        };
                        required_providers.push(required_provider);
                    }
                }
            }
        }
    }
    Ok(required_providers)
}

fn split_expr(expr: &Expression, outer_key: &str) -> Vec<(String, String)> {
    match expr {
        Expression::Object(map) => map
            .iter()
            .map(|(k, v)| {
                // turn the ObjectKey into a String
                let field = match k {
                    ObjectKey::Identifier(id) => id.clone().to_string(),
                    ObjectKey::Expression(inner) => expr_to_string(inner),
                    _ => panic!("unsupported ObjectKey in required_providers: {:?}", k),
                };
                (field, expr_to_string(v))
            })
            .collect(),

        // everything else is a simple single key -> single value
        other => vec![(outer_key.to_string(), expr_to_string(other))],
    }
}

/// Stringify a single HCL Expression into its "value"
/// (no extra JSON quotes, objects/arrays flattened).
fn expr_to_string(expr: &Expression) -> String {
    match expr {
        Expression::String(s) => s.clone(),
        Expression::Variable(v) => v.to_string(),
        Expression::Bool(b) => b.to_string(),
        Expression::Number(n) => n.to_string(),
        Expression::Null => "null".to_string(),
        Expression::TemplateExpr(te) => te.to_string(),

        // arrays become “[elem1, elem2, …]”
        Expression::Array(arr) => {
            let items = arr
                .iter()
                .map(expr_to_string)
                .collect::<Vec<_>>()
                .join(", ");
            format!("[{}]", items)
        }

        other => panic!("unsupported expression in required_providers: {:?}", other),
    }
}

fn get_attributes(block: &Block, excluded_attrs: Vec<String>) -> HashMap<String, String> {
    let mut attrs = HashMap::new();
    for attr in block.body().attributes() {
        if excluded_attrs.contains(&attr.key().to_string()) {
            continue;
        }
        for (k, v) in split_expr(&attr.expr, &attr.key()) {
            attrs.insert(k, v);
        }
    }
    attrs
}

#[allow(dead_code)]
pub fn indent(s: &str, level: usize) -> String {
    let indent = "  ".repeat(level);
    s.lines()
        .map(|line| format!("{}{}", indent, line))
        .collect::<Vec<String>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn test_get_variable_block_string() {
        let variables_str = r#"
variable "bucket_name" {
  type = string
  default = "some-bucket-name"
}
"#;
        assert_eq!(
            *get_variables_from_tf_files(variables_str)
                .unwrap()
                .first()
                .unwrap(),
            TfVariable {
                name: "bucket_name".to_string(),
                _type: serde_json::json!("string"),
                default: serde_json::json!("some-bucket-name"),
                description: "".to_string(),
                nullable: true,
                sensitive: false,
            }
        );
    }

    #[test]
    fn test_get_variable_block_map_string() {
        let variables_str = r#"
variable "tags" {
  type = map(string)
  default = {
    "tag_environment" = "some_value1"
    "tag_name" = "some_value2"
  }
}
"#;
        assert_eq!(
            *get_variables_from_tf_files(variables_str)
                .unwrap()
                .first()
                .unwrap(),
            TfVariable {
                name: "tags".to_string(),
                _type: serde_json::json!("map(string)"),
                default: serde_json::json!({
                    "tag_environment": "some_value1",
                    "tag_name": "some_value2"
                }),
                description: "".to_string(),
                nullable: true,
                sensitive: false,
            }
        );
    }

    #[test]
    fn test_get_variable_block_map_string_no_default() {
        let variables_str = r#"
variable "tags" {
  type = map(string)
}
"#;
        assert_eq!(
            *get_variables_from_tf_files(variables_str)
                .unwrap()
                .first()
                .unwrap(),
            TfVariable {
                name: "tags".to_string(),
                _type: serde_json::json!("map(string)"),
                default: serde_json::json!(null),
                description: "".to_string(),
                nullable: true,
                sensitive: false,
            }
        );
    }

    #[test]
    fn test_get_variable_block_set_string_no_default() {
        let variables_str = r#"
variable "tags" {
  type = set(string)
}
"#;
        assert_eq!(
            *get_variables_from_tf_files(variables_str)
                .unwrap()
                .first()
                .unwrap(),
            TfVariable {
                name: "tags".to_string(),
                _type: serde_json::json!("set(string)"),
                default: serde_json::json!(null),
                description: "".to_string(),
                nullable: true,
                sensitive: false,
            }
        );
    }

    #[test]
    fn test_get_required_provider_aws() {
        let required_providers_str = r#"
terraform {
  required_providers {
    aws = {
      source = "hashicorp/aws"
      version = "~> 5.0"
    }
  }
}
"#;
        assert_eq!(
            *get_tf_required_providers_from_tf_files(required_providers_str).unwrap(),
            [TfRequiredProvider {
                name: "aws".to_string(),
                source: "hashicorp/aws".to_string(),
                version: "~> 5.0".to_string(),
            }]
        );
    }

    #[test]
    fn test_get_required_provider_aws_and_kubernetes() {
        let required_providers_str = r#"
terraform {
  required_providers {
    aws = {
      source = "hashicorp/aws"
      version = "~> 5.0"
    }
    kubernetes = {
      source = "hashicorp/kubernetes"
      version = "2.36.0"
    }
  }
}
"#;
        assert_eq!(
            *get_tf_required_providers_from_tf_files(required_providers_str).unwrap(),
            [
                TfRequiredProvider {
                    name: "aws".to_string(),
                    source: "hashicorp/aws".to_string(),
                    version: "~> 5.0".to_string(),
                },
                TfRequiredProvider {
                    name: "kubernetes".to_string(),
                    source: "hashicorp/kubernetes".to_string(),
                    version: "2.36.0".to_string(),
                }
            ]
        );
    }

    #[test]
    fn test_get_required_provider_empty() {
        let required_providers_str = "";
        assert_eq!(
            *get_tf_required_providers_from_tf_files(required_providers_str).unwrap(),
            []
        );
    }
}
