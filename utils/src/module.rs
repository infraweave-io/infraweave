use env_defs::TfLockProvider;
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
pub fn validate_tf_backend_not_set(contents: &str) -> Result<(), anyhow::Error> {
    let parsed_hcl: HashMap<String, serde_json::Value> =
        de::from_str(contents).map_err(|err| anyhow::anyhow!("Failed to parse HCL: {}", err))?;

    if let Some(terraform_blocks) = parsed_hcl.get("terraform") {
        if terraform_blocks.is_object() {
            ensure_no_backend_block(terraform_blocks)?;
        } else if terraform_blocks.is_array() {
            for terraform_block in terraform_blocks.as_array().unwrap() {
                ensure_no_backend_block(terraform_block)?;
            }
        } else {
            return Ok(());
        }
    }

    Ok(())
}

#[allow(dead_code)]
pub fn get_providers_from_lockfile(contents: &str) -> Result<Vec<TfLockProvider>, anyhow::Error> {
    let parsed_hcl: HashMap<String, serde_json::Value> =
        de::from_str(contents).map_err(|err| anyhow::anyhow!("Failed to parse HCL: {}", err))?;

    let providers: Vec<TfLockProvider> = parsed_hcl
        .get("provider")
        .map(|v| {
            if v.is_object() {
                v.as_object()
                    .unwrap()
                    .iter()
                    .map(|(k, v)| TfLockProvider {
                        source: k.to_string(),
                        version: v
                            .get("version")
                            .and_then(|s| s.as_str())
                            .unwrap()
                            .to_string(),
                    })
                    .collect::<Vec<_>>()
            } else if v.is_array() {
                v.as_array()
                    .unwrap()
                    .iter()
                    .flat_map(|provider| {
                        provider
                            .as_object()
                            .unwrap()
                            .iter()
                            .map(|(k, v)| TfLockProvider {
                                source: k.to_string(),
                                version: v
                                    .get("version")
                                    .and_then(|s| s.as_str())
                                    .unwrap()
                                    .to_string(),
                            })
                            .collect::<Vec<_>>()
                    })
                    .collect::<Vec<_>>()
            } else {
                panic!("Expected a JSON object or array for provider block");
            }
        })
        .unwrap_or_default();

    Ok(providers)
}

#[allow(dead_code)]
pub fn validate_tf_required_providers_is_set(
    required_providers: &Vec<TfRequiredProvider>,
    expected_providers: &Vec<TfLockProvider>,
) -> Result<(), anyhow::Error> {
    let mut expected_providers = expected_providers.clone();

    for provider in required_providers {
        expected_providers.retain(|x| {
            x.source != provider.source
                && *x.source != format!("registry.terraform.io/{}", provider.source)
        });
    }

    if !expected_providers.is_empty() {
        return Err(anyhow::anyhow!(
            "required_providers block is missing following entries ({}) in the terraform configuration\n{}",
            expected_providers.iter().map(|f| f.source.clone()).collect::<Vec<_>>().join(", "),
            get_required_providers_block_help(&serde_json::json!({}))
        ));
    }

    Ok(())
}

#[allow(dead_code)]
pub fn validate_tf_extra_environment_variables(
    extra_environment_variables: &Vec<String>,
    tf_variables: &Vec<TfVariable>,
) -> Result<(), anyhow::Error> {
    const VALID_EXTRA_ENVIRONMENT_VARIABLES: &[&str] = &[
        // Generic (always have a value during runtime)
        "INFRAWEAVE_DEPLOYMENT_ID",
        "INFRAWEAVE_ENVIRONMENT",
        "INFRAWEAVE_REFERENCE",
        "INFRAWEAVE_MODULE_VERSION",
        "INFRAWEAVE_MODULE_TYPE",
        "INFRAWEAVE_MODULE_TRACK",
        "INFRAWEAVE_DRIFT_DETECTION",
        "INFRAWEAVE_DRIFT_DETECTION_INTERVAL",
        // GitHub specific (only have a value if pushed to GitHub) TODO: reuse for GitLab
        "INFRAWEAVE_GIT_COMMITTER_EMAIL",
        "INFRAWEAVE_GIT_COMMITTER_NAME",
        "INFRAWEAVE_GIT_ACTOR_USERNAME",
        "INFRAWEAVE_GIT_ACTOR_PROFILE_URL",
        "INFRAWEAVE_GIT_REPOSITORY_NAME",
        "INFRAWEAVE_GIT_REPOSITORY_PATH",
        "INFRAWEAVE_GIT_COMMIT_SHA",
    ];
    for tf_variable in tf_variables {
        if extra_environment_variables.contains(&tf_variable.name) {
            if tf_variable.default != Some(serde_json::json!("")) {
                return Err(anyhow::anyhow!(
                    "Extra environment variable {} must set default value to \"\"",
                    tf_variable.name
                ));
            }
            if tf_variable._type != "string" {
                return Err(anyhow::anyhow!(
                    "Extra environment variable {} must be of type string",
                    tf_variable.name
                ));
            }
        }
        if tf_variable.name.starts_with("INFRAWEAVE_")
            && !VALID_EXTRA_ENVIRONMENT_VARIABLES.contains(&tf_variable.name.as_str())
        {
            return Err(anyhow::anyhow!(
                "Extra environment variable {} (starting with \"INFRAWEAVE_\") is not a valid extra environment variable.\nValid extra environment variables are: {}",
                tf_variable.name, VALID_EXTRA_ENVIRONMENT_VARIABLES.iter().map(|s| s.to_string()).collect::<Vec<_>>().join(", ")
            ));
        }
    }
    Ok(())
}

fn ensure_no_backend_block(terraform_block: &serde_json::Value) -> Result<(), anyhow::Error> {
    // Check if the backend block is present in the terraform configuration
    if let Some(_backend_blocks) = terraform_block.get("backend") {
        return Err(anyhow::anyhow!(
            "Backend block was found in the terraform backend configuration\n{}",
            get_backend_block_help(terraform_block)
        ));
    }
    Ok(())
}

pub fn get_backend_block_help(block: &serde_json::Value) -> String {
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

pub fn get_required_providers_block_help(block: &serde_json::Value) -> String {
    let help = format!(
        r#"
Please make sure you set required_providers block in your terraform code, this ensures you are in control of the versions of the providers are using.

Please make any required changes terraform configuration to proceed:

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
                let default_value: Option<serde_json::Value> = var_attrs.get("default").cloned();
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

                        let source = attrs
                            .get("source")
                            .expect(&format!(
                                "source is missing in {} in required_providers",
                                required_provider_name
                            ))
                            .to_string();
                        // If only have one / then assume it is a registry
                        let source = if source.matches('/').count() == 1 {
                            format!("registry.terraform.io/{}", source)
                        } else {
                            source
                        };

                        let required_provider = TfRequiredProvider {
                            name: required_provider_name.clone(),
                            source: source,
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
                default: Some(serde_json::json!("some-bucket-name")),
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
                default: Some(serde_json::json!({
                    "tag_environment": "some_value1",
                    "tag_name": "some_value2"
                })),
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
                default: None,
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
                default: None,
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
                source: "registry.terraform.io/hashicorp/aws".to_string(),
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
                    source: "registry.terraform.io/hashicorp/aws".to_string(),
                    version: "~> 5.0".to_string(),
                },
                TfRequiredProvider {
                    name: "kubernetes".to_string(),
                    source: "registry.terraform.io/hashicorp/kubernetes".to_string(),
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

    #[test]
    fn test_validate_tf_backend_not_set() {
        let required_providers_str = "";
        assert_eq!(
            validate_tf_backend_not_set(required_providers_str).is_ok(),
            true
        );
    }

    #[test]
    fn test_validate_tf_backend_not_set_not_ok() {
        let required_providers_str = r#"
        terraform {
           backend "s3" {}
        }
        "#;
        assert_eq!(
            validate_tf_backend_not_set(required_providers_str).is_ok(),
            false
        );
    }

    #[test]
    fn test_validate_tf_required_providers_is_set() {
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
        let required_providers =
            get_tf_required_providers_from_tf_files(required_providers_str).unwrap();
        let expected_providers = vec![
            // This is extracted from the lockfile
            TfLockProvider {
                source: "registry.terraform.io/hashicorp/aws".to_string(),
                version: "5.81.0".to_string(),
            },
            TfLockProvider {
                source: "registry.terraform.io/hashicorp/kubernetes".to_string(),
                version: "2.36.0".to_string(),
            },
        ];
        let res = validate_tf_required_providers_is_set(&required_providers, &expected_providers);
        assert_eq!(res.is_ok(), true);
    }

    #[test]
    fn test_validate_tf_required_providers_with_custom_provider() {
        let required_providers_str = r#"
terraform {
  required_providers {
    mycustom = {
      source  = "app.terraform.io/my-org/my-custom-provider"
      version = "1.2.3"
    }
  }
}
"#;
        let required_providers =
            get_tf_required_providers_from_tf_files(required_providers_str).unwrap();
        let expected_providers = vec![
            // This is extracted from the lockfile
            TfLockProvider {
                source: "app.terraform.io/my-org/my-custom-provider".to_string(),
                version: "1.2.3".to_string(),
            },
        ];

        let res = validate_tf_required_providers_is_set(&required_providers, &expected_providers);
        assert_eq!(res.is_ok(), true);
    }

    #[test]
    fn test_validate_tf_required_providers_is_set_not_ok() {
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
        let required_providers =
            get_tf_required_providers_from_tf_files(required_providers_str).unwrap();
        let expected_providers = vec![
            // This is extracted from the lockfile
            TfLockProvider {
                source: "registry.terraform.io/hashicorp/aws".to_string(),
                version: "5.81.0".to_string(),
            },
            TfLockProvider {
                source: "registry.terraform.io/hashicorp/kubernetes".to_string(),
                version: "2.36.0".to_string(),
            },
        ];
        let res = validate_tf_required_providers_is_set(&required_providers, &expected_providers);
        assert_eq!(res.is_err(), true);
    }

    #[test]
    fn test_get_providers_from_lockfile() {
        let lockfile_str = r#"
        # This file is maintained automatically by "terraform init".
# Manual edits may be lost in future updates.

provider "registry.terraform.io/hashicorp/aws" {
  version = "5.81.0"
  hashes = [
    "h1:YoOBDt9gdoivbUh1iGoZNqRBUdBO+PBAxpSZFeTLLYE=",
    "zh:05534adf6f02d6ec26dbeb37a4d2b6edb63f12dc9ab5cc05ab89329fcd793194",
    "zh:cdf524a269b4aeb5b1f081d91f54bae967ad50d9c392073a0db1602166a48dff",
  ]
}
"#;
        assert_eq!(
            get_providers_from_lockfile(lockfile_str).unwrap(),
            vec![TfLockProvider {
                source: "registry.terraform.io/hashicorp/aws".to_string(),
                version: "5.81.0".to_string(),
            }]
        );
    }

    #[test]
    fn test_get_providers_from_lockfile_multiple() {
        let lockfile_str = r#"
# This file is maintained automatically by "terraform init".
# Manual edits may be lost in future updates.

provider "registry.terraform.io/hashicorp/aws" {
  version = "5.81.0"
  hashes = [
    "h1:YoOBDt9gdoivbUh1iGoZNqRBUdBO+PBAxpSZFeTLLYE=",
    "zh:05534adf6f02d6ec26dbeb37a4d2b6edb63f12dc9ab5cc05ab89329fcd793194",
    "zh:cdf524a269b4aeb5b1f081d91f54bae967ad50d9c392073a0db1602166a48dff",
  ]
}

provider "registry.terraform.io/hashicorp/kubernetes" {
  version     = "2.36.0"
  constraints = "2.36.0"
  hashes = [
    "h1:94wlXkBzfXwyLVuJVhMdzK+VGjFnMjdmFkYhQ1RUFhI=",
    "zh:07f38fcb7578984a3e2c8cf0397c880f6b3eb2a722a120a08a634a607ea495ca",
    "zh:f688b9ec761721e401f6859c19c083e3be20a650426f4747cd359cdc079d212a",
  ]
}

"#;
        assert_eq!(
            get_providers_from_lockfile(lockfile_str).unwrap(),
            vec![
                TfLockProvider {
                    source: "registry.terraform.io/hashicorp/aws".to_string(),
                    version: "5.81.0".to_string(),
                },
                TfLockProvider {
                    source: "registry.terraform.io/hashicorp/kubernetes".to_string(),
                    version: "2.36.0".to_string(),
                }
            ]
        );
    }

    #[test]
    fn test_validate_tf_extra_environment_variables() {
        let extra_environment_variables = vec!["INFRAWEAVE_DEPLOYMENT_ID".to_string()];
        let tf_variables = vec![
            TfVariable {
                name: "bucket_name".to_string(),
                _type: serde_json::json!("string"),
                default: Some(serde_json::json!("")),
                description: "".to_string(),
                nullable: true,
                sensitive: false,
            },
            TfVariable {
                name: "INFRAWEAVE_DEPLOYMENT_ID".to_string(),
                _type: serde_json::json!("string"),
                default: Some(serde_json::json!("")),
                description: "Some description maybe".to_string(),
                nullable: true,
                sensitive: false,
            },
        ];

        assert_eq!(
            validate_tf_extra_environment_variables(&extra_environment_variables, &tf_variables)
                .is_ok(),
            true
        );
    }

    #[test]
    fn test_validate_tf_extra_environment_variables_invalid_value() {
        let extra_environment_variables = vec!["INFRAWEAVE_DEPLOYMENT_ID".to_string()];
        let tf_variables = vec![
            TfVariable {
                name: "bucket_name".to_string(),
                _type: serde_json::json!("string"),
                default: Some(serde_json::json!("")),
                description: "".to_string(),
                nullable: true,
                sensitive: false,
            },
            TfVariable {
                name: "INFRAWEAVE_DEPLOYMENT_ID".to_string(),
                _type: serde_json::json!("string"),
                default: Some(serde_json::json!("some_value_here_not_allowed")),
                description: "Some description maybe".to_string(),
                nullable: true,
                sensitive: false,
            },
        ];

        assert_eq!(
            validate_tf_extra_environment_variables(&extra_environment_variables, &tf_variables)
                .is_err(),
            true
        );
    }

    #[test]
    fn test_validate_tf_extra_environment_variables_invalid_type() {
        let extra_environment_variables = vec!["INFRAWEAVE_DEPLOYMENT_ID".to_string()];
        let tf_variables = vec![
            TfVariable {
                name: "bucket_name".to_string(),
                _type: serde_json::json!("string"),
                default: Some(serde_json::json!("")),
                description: "".to_string(),
                nullable: true,
                sensitive: false,
            },
            TfVariable {
                name: "INFRAWEAVE_DEPLOYMENT_ID".to_string(),
                _type: serde_json::json!("bool"),
                default: Some(serde_json::json!("")),
                description: "Some description maybe".to_string(),
                nullable: true,
                sensitive: false,
            },
        ];

        assert_eq!(
            validate_tf_extra_environment_variables(&extra_environment_variables, &tf_variables)
                .is_err(),
            true
        );
    }
}
