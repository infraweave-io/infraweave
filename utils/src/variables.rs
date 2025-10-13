use env_defs::{DeploymentManifest, ModuleResp};

pub fn verify_variable_claim_casing(
    claim: &DeploymentManifest,
    provided_variables: &serde_json::Value,
) -> Result<(), anyhow::Error> {
    // Check that provided variable names match the original casing exactly
    if let Some(provided_vars) = provided_variables.as_object() {
        for provided_name in provided_vars.keys() {
            // Convert the provided key to camelCase
            let camel_case = crate::to_camel_case(provided_name);
            // If the conversion changes the key, it indicates snake_case formatting.
            if provided_name != &camel_case {
                return Err(anyhow::anyhow!(
                    "Variable name casing mismatch in claim '{}': Provided '{}', expected '{}'",
                    claim.metadata.name.clone(),
                    provided_name.clone(),
                    camel_case,
                ));
            }
        }
    }
    Ok(())
}

pub fn verify_variable_existence_and_type(
    module: &ModuleResp,
    variables: &serde_json::Value,
) -> Result<(), anyhow::Error> {
    let variables_map = variables
        .as_object()
        .ok_or_else(|| anyhow::anyhow!("Expected variables to be a JSON object"))?;

    let mut errors = Vec::new();

    let re = regex::Regex::new(r"\{\{\s*(\w+)::(\w+)::(\w+)\s*\}\}").unwrap();
    for (variable_key, variable_value) in variables_map {
        match module.tf_variables.iter().find(|v| v.name == *variable_key) {
            Some(module_variable) => {
                let variable_value_type = match variable_value {
                    serde_json::Value::String(_) => "string",
                    serde_json::Value::Number(_) => "number",
                    serde_json::Value::Bool(_) => "bool",
                    serde_json::Value::Array(_) => "list",
                    serde_json::Value::Null => "null",
                    serde_json::Value::Object(_) => "object",
                };

                let module_variable_type = match &module_variable._type {
                    serde_json::Value::Bool(_) => "bool",
                    serde_json::Value::Number(_) => "number",
                    serde_json::Value::String(val) => {
                        if val.starts_with("map(") {
                            // Covers map(string), map(number), etc.
                            "object"
                        } else if val.starts_with("list(") || val.starts_with("set(") {
                            // Covers list(string), list(number), etc.
                            "list"
                        } else {
                            &val.to_string()
                        }
                    }
                    serde_json::Value::Array(_) => "list",
                    serde_json::Value::Object(_) => "object",
                    serde_json::Value::Null => "null",
                };

                let is_reference = variable_value.as_str().is_some_and(|s| re.is_match(s));

                if variable_value_type != module_variable_type {
                    if is_reference {
                        log::warn!("
                            Variable \"{}\" is a reference and its type is not checked since output type of reference cannot be implied. Please ensure it matches the expected type.",
                            variable_key
                        );
                    } else if variable_value_type == "null" && module_variable.nullable {
                        // This is valid when a user explicitly wants to set a nullable variable to null
                        log::debug!(
                            "Variable \"{}\" is set to null, which is allowed because it is nullable",
                            variable_key
                        );
                    } else {
                        errors.push(format!(
                            "Variable \"{}\" is of type {} but should be of type {}",
                            variable_key, variable_value_type, module_variable_type
                        ));
                    }
                }
            }
            None => {
                errors.push(format!(
                    "Variable \"{}\" not found in this {} version ({}). Please check the documentation for available variables",
                    variable_key, module.module_type, module.version
                ));
            }
        }
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(anyhow::anyhow!(errors.join("; ")))
    }
}

pub fn verify_required_variables_are_set(
    module: &ModuleResp,
    variables: &serde_json::Value,
) -> Result<(), anyhow::Error> {
    let mut missing_variables = vec![];
    let module_variables = &module.tf_variables;
    let variables_map = variables.as_object().unwrap();
    for variable in module_variables {
        if variable.nullable && variable.default == Some(serde_json::Value::Null) {
            // If the variable is nullable and has a default value, it is not required
            continue;
        }
        if variable.default.is_some() && variable.default != Some(serde_json::Value::Null) {
            // If the variable has a default value, it is not required anyway
            continue;
        }
        // If the variable is not nullable and has no default value, it is required;
        // Ensure the variable is set
        if !variables_map.contains_key(variable.name.as_str()) {
            missing_variables.push(variable.name.clone());
        }
    }

    if !missing_variables.is_empty() {
        let plural = if missing_variables.len() > 1 { "s" } else { "" };
        return Err(anyhow::anyhow!(
            "Missing required variable{}: \"{}\"",
            plural,
            missing_variables.join("\", \"")
        ));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use env_defs::{Metadata, ModuleManifest, ModuleSpec, TfOutput, TfVariable};
    use serde_json::Value;

    #[test]
    fn test_variables_in_claim() {
        let module = s3bucket_module();
        let variables = serde_json::json!({
            "bucket_name": "my-unique-bucket-name",
            "tags": {
                "Name234": "my-s3bucket",
                "Environment43": "dev"
            }
        });

        let result = verify_variable_existence_and_type(&module, &variables);
        assert!(result.is_ok());
    }

    #[test]
    fn test_variables_in_claim_reference() {
        let module = s3bucket_module();
        let variables = serde_json::json!({
            "bucket_name": "my-unique-bucket-name",
            "tags": "{{ S3Bucket::bucket2::tags }}"
        });

        let result = verify_variable_existence_and_type(&module, &variables);
        assert!(result.is_ok());
    }

    #[test]
    fn test_missing_variable_in_claim() {
        let module = s3bucket_module();
        let variables = serde_json::json!({
            "bucket_name": "my-unique-bucket-name",
            "this_variable_does_not_exist": "some_value"
        });

        let result = verify_variable_existence_and_type(&module, &variables);
        assert!(result.is_err());
    }

    #[test]
    fn test_all_required_variables_are_set_1() {
        let module = s3bucket_module();
        let variables = serde_json::json!({
            "bucket_name": "my-unique-bucket-name",
            "enable_acl": false,
            // "nullable_with_default" is not set, it's nullable but has default value null, meaning Some(serde_json::Value::Null) => all good
            "nullable_without_default": "some_value",
        });

        let result = verify_required_variables_are_set(&module, &variables);
        assert!(result.is_ok());
    }

    #[test]
    fn test_all_required_variables_are_set_2() {
        let module = s3bucket_module();
        let variables = serde_json::json!({
            "bucket_name": "my-unique-bucket-name",
            "enable_acl": false,
            "nullable_with_default": Some(serde_json::Value::Null),
            "nullable_without_default": "some_value",
        });

        let result = verify_required_variables_are_set(&module, &variables);
        assert!(result.is_ok());
    }

    #[test]
    fn test_all_required_variables_are_set_3() {
        let module = s3bucket_module();
        let variables = serde_json::json!({
            "bucket_name": "my-unique-bucket-name",
            "enable_acl": false,
            "nullable_with_default": Some(serde_json::Value::Null),
            "nullable_without_default": Some(serde_json::Value::Null),
        });

        let result = verify_required_variables_are_set(&module, &variables);
        assert!(result.is_ok());
    }

    #[test]
    fn test_all_required_variables_are_set_except_one() {
        let module = s3bucket_module();
        let variables = serde_json::json!({
            "bucket_name": "my-unique-bucket-name",
            "enable_acl": false,
            // "nullable_with_default" is not set, it's nullable but has default value null, meaning Some(serde_json::Value::Null) => all good
            // "nullable_without_default" is not set, it's nullable and has no default value, meaning None => this is an error
        });

        let result = verify_required_variables_are_set(&module, &variables);
        assert!(result.is_err());
    }

    #[test]
    fn test_all_required_variables_are_not_set() {
        let module = s3bucket_module();
        let variables = serde_json::json!({
            "tags": {
                "Name234": "my-s3bucket",
                "Environment43": "dev"
            }
        });

        let result = verify_required_variables_are_set(&module, &variables);
        assert!(result.is_err());
    }

    #[test]
    fn test_valid_variable_types() {
        let module = s3bucket_module();
        let variables = serde_json::json!({
            "bucket_name": "my-unique-bucket-name",
            "enable_acl": false,
            "tags": {
                "Name234": "my-s3bucket",
                "Environment43": "dev"
            }
        });

        let result = verify_variable_existence_and_type(&module, &variables);
        assert!(result.is_ok());
    }

    #[test]
    fn test_invalid_variable_types_int() {
        let module = s3bucket_module();
        let variables = serde_json::json!({
            "bucket_name": "my-unique-bucket-name",
            "enable_acl": 123, // Should be bool
            "tags": {
                "Name234": "my-s3bucket",
                "Environment43": "dev"
            }
        });

        let result = verify_variable_existence_and_type(&module, &variables);
        assert!(result.is_err());
    }

    #[test]
    fn test_invalid_variable_types_string() {
        let module = s3bucket_module();
        let variables = serde_json::json!({
            "enable_acl": "false_should_be_bool",
        });

        let result = verify_variable_existence_and_type(&module, &variables);
        assert!(result.is_err());
    }

    #[test]
    fn test_invalid_variable_types_map1() {
        let module = s3bucket_module();
        let variables = serde_json::json!({
            "bucket_name": {
                "this": "is",
                "not_a": "string",
            },
        });

        let result = verify_variable_existence_and_type(&module, &variables);
        assert!(result.is_err());
    }

    #[test]
    fn test_invalid_variable_types_map2() {
        let module = s3bucket_module();
        let variables = serde_json::json!({
            "tags": "not_a_map",
        });

        let result = verify_variable_existence_and_type(&module, &variables);
        assert!(result.is_err());
    }

    #[test]
    fn test_nullable_variable_with_default_set_to_null() {
        // Create a module with a nullable string variable that has a default value
        let module = ModuleResp {
            oci_artifact_set: None,
            s3_key: "test/test-0.1.0.zip".to_string(),
            track: "dev".to_string(),
            track_version: "dev#000.001.000".to_string(),
            version: "0.1.0".to_string(),
            timestamp: "2024-10-10T22:23:14.368+02:00".to_string(),
            module_name: "TestNullable".to_string(),
            module_type: "module".to_string(),
            module: "testnullable".to_string(),
            description: "Test module".to_string(),
            reference: "https://github.com/test/test".to_string(),
            manifest: ModuleManifest {
                metadata: Metadata {
                    name: "testnullable".to_string(),
                },
                api_version: "infraweave.io/v1".to_string(),
                kind: "Module".to_string(),
                spec: ModuleSpec {
                    module_name: "TestNullable".to_string(),
                    version: Some("0.1.0".to_string()),
                    description: "Test module".to_string(),
                    reference: "https://github.com/test/test".to_string(),
                    examples: None,
                    cpu: None,
                    memory: None,
                },
            },
            tf_outputs: vec![],
            tf_variables: vec![
                TfVariable {
                    name: "my_var".to_string(),
                    _type: Value::String("string".to_string()),
                    default: Some(serde_json::json!("standard")),
                    description: "A nullable variable with a default value".to_string(),
                    nullable: true,
                    sensitive: false,
                },
                TfVariable {
                    name: "another_var".to_string(),
                    _type: Value::String("string".to_string()),
                    default: None,
                    description: "A required non-nullable variable".to_string(),
                    nullable: false,
                    sensitive: false,
                },
            ],
            tf_extra_environment_variables: vec![],
            tf_required_providers: vec![],
            tf_lock_providers: vec![],
            stack_data: None,
            version_diff: None,
            cpu: "1024".to_string(),
            memory: "4096".to_string(),
        };

        // Test that setting a nullable variable to null is allowed
        let variables = serde_json::json!({
            "my_var": null,
            "another_var": "required-value"
        });

        let result = verify_variable_existence_and_type(&module, &variables);
        // This should succeed because my_var is nullable, even though its type is string
        assert!(
            result.is_ok(),
            "Should allow null for nullable variable with default value. Error: {:?}",
            result.err()
        );
    }

    #[test]
    fn test_non_nullable_variable_set_to_null_fails() {
        // Create a module with a NON-nullable string variable
        let module = ModuleResp {
            oci_artifact_set: None,
            s3_key: "test/test-0.1.0.zip".to_string(),
            track: "dev".to_string(),
            track_version: "dev#000.001.000".to_string(),
            version: "0.1.0".to_string(),
            timestamp: "2024-10-10T22:23:14.368+02:00".to_string(),
            module_name: "TestNullable".to_string(),
            module_type: "module".to_string(),
            module: "testnullable".to_string(),
            description: "Test module".to_string(),
            reference: "https://github.com/test/test".to_string(),
            manifest: ModuleManifest {
                metadata: Metadata {
                    name: "testnullable".to_string(),
                },
                api_version: "infraweave.io/v1".to_string(),
                kind: "Module".to_string(),
                spec: ModuleSpec {
                    module_name: "TestNullable".to_string(),
                    version: Some("0.1.0".to_string()),
                    description: "Test module".to_string(),
                    reference: "https://github.com/test/test".to_string(),
                    examples: None,
                    cpu: None,
                    memory: None,
                },
            },
            tf_outputs: vec![],
            tf_variables: vec![TfVariable {
                name: "my_var".to_string(),
                _type: Value::String("string".to_string()),
                default: None,
                description: "A non-nullable required variable".to_string(),
                nullable: false,
                sensitive: false,
            }],
            tf_extra_environment_variables: vec![],
            tf_required_providers: vec![],
            tf_lock_providers: vec![],
            stack_data: None,
            version_diff: None,
            cpu: "1024".to_string(),
            memory: "4096".to_string(),
        };

        // Test that setting a non-nullable variable to null fails
        let variables = serde_json::json!({
            "my_var": null
        });

        let result = verify_variable_existence_and_type(&module, &variables);
        // This should fail because my_var is NOT nullable
        assert!(
            result.is_err(),
            "Should reject null for non-nullable variable"
        );
        let error_msg = format!("{:?}", result.err());
        assert!(
            error_msg.contains("is of type null but should be of type string"),
            "Error message should mention type mismatch"
        );
    }

    fn s3bucket_module() -> ModuleResp {
        ModuleResp {
            oci_artifact_set: None,
            s3_key: "s3bucket/s3bucket-0.0.21.zip".to_string(),
            track: "dev".to_string(),
            track_version: "dev#000.000.021".to_string(),
            version: "0.0.21".to_string(),
            timestamp: "2024-10-10T22:23:14.368+02:00".to_string(),
            module_name: "S3Bucket".to_string(),
            module_type: "module".to_string(),
            module: "s3bucket".to_string(),
            description: "Some description...".to_string(),
            reference: "https://github.com/infreweave-io/modules/s3bucket".to_string(),
            manifest: ModuleManifest {
                metadata: Metadata {
                    name: "metadata".to_string(),
                },
                api_version: "infraweave.io/v1".to_string(),
                kind: "Module".to_string(),
                spec: ModuleSpec {
                    module_name: "S3Bucket".to_string(),
                    version: Some("0.0.21".to_string()),
                    description: "Some description...".to_string(),
                    reference: "https://github.com/infreweave-io/modules/s3bucket".to_string(),
                    examples: None,
                    cpu: None,
                    memory: None,
                },
            },
            tf_outputs: vec![
                TfOutput {
                    name: "bucket_arn".to_string(),
                    description: "ARN of the bucket".to_string(),
                    value: "".to_string(),
                },
                // TfOutput { name: "region".to_string(), description: "".to_string(), value: "".to_string() },
                // TfOutput { name: "sse_algorithm".to_string(), description: "".to_string(), value: "".to_string() },
            ],
            tf_variables: vec![
                TfVariable {
                    default: None,
                    name: "bucket_name".to_string(),
                    description: "Name of the S3 bucket".to_string(),
                    _type: Value::String("string".to_string()),
                    nullable: false,
                    sensitive: false,
                },
                TfVariable {
                    default: Some(serde_json::Value::Null),
                    name: "enable_acl".to_string(),
                    description: "Enable ACL for the S3 bucket".to_string(),
                    _type: Value::Bool(false),
                    nullable: false,
                    sensitive: false,
                },
                TfVariable {
                    default: serde_json::from_value(
                        serde_json::json!({"Test": "hej", "AnotherTag": "something"}),
                    )
                    .unwrap(),
                    name: "tags".to_string(),
                    description: "Tags to apply to the S3 bucket".to_string(),
                    _type: Value::String("map(string)".to_string()),
                    nullable: true,
                    sensitive: false,
                },
                TfVariable {
                    default: Some(serde_json::Value::Null), // This is set to null
                    name: "nullable_with_default".to_string(),
                    description: "A variable that is nullable=true but has no default value set, this means it is required".to_string(),
                    _type: Value::Null,
                    nullable: true,
                    sensitive: false,
                },
                TfVariable {
                    default: None, // This is not set
                    name: "nullable_without_default".to_string(),
                    description: "A variable that is nullable=true and has default value set, this means it is not required".to_string(),
                    _type: Value::String("string".to_string()),
                    nullable: true,
                    sensitive: false,
                },
            ],
            tf_extra_environment_variables: vec![],
            tf_required_providers: vec![],
            tf_lock_providers: vec![],
            stack_data: None,
            version_diff: None,
            cpu: "1024".to_string(),
            memory: "2048".to_string(),
        }
    }
}
