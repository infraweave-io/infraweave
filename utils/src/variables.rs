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
        match module
            .tf_variables
            .iter()
            .chain(
                module
                    .tf_providers
                    .iter()
                    .flat_map(|provider| provider.tf_variables.iter()),
            )
            .find(|v| v.name == *variable_key)
        {
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
                        if val.starts_with("map(") || val.starts_with("object(") {
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

                if module_variable_type == "any" {
                    continue;
                }

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
    let mut provider_variables = module
        .tf_providers
        .iter()
        .flat_map(|p| p.tf_variables.iter())
        .collect::<Vec<_>>();
    provider_variables.sort_by_key(|v| &v.name);
    provider_variables.dedup_by_key(|v| &v.name);
    let provider_variables = provider_variables;
    let variables_map = variables.as_object().unwrap();
    for variable in module_variables.iter().chain(provider_variables) {
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

/// Verifies that variable names from Terraform can survive a roundtrip conversion
pub fn verify_variable_name_roundtrip(
    tf_variables: &[env_defs::TfVariable],
) -> Result<(), anyhow::Error> {
    let mut errors = Vec::new();

    for var in tf_variables {
        let original_name = &var.name;

        // Skip INFRAWEAVE_ prefixed variables as they are special environment variables
        if original_name.starts_with("INFRAWEAVE_") {
            continue;
        }

        // Perform roundtrip: snake_case -> camelCase -> snake_case
        let camel_case = crate::to_camel_case(original_name);
        let back_to_snake = crate::to_snake_case(&camel_case);

        if original_name != &back_to_snake {
            errors.push(format!(
                "Variable '{}' fails roundtrip case conversion: '{}' -> '{}' -> '{}'. \
                Variables must use snake_case naming (e.g., 'my_variable', 'user_count') \
                to ensure proper conversion to camelCase for the API.",
                original_name, original_name, camel_case, back_to_snake
            ));
        }
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(anyhow::anyhow!(
            "Variable name roundtrip verification failed:\n{}",
            errors.join("\n")
        ))
    }
}

/// Verifies that output names from Terraform can survive a roundtrip conversion
pub fn verify_output_name_roundtrip(
    tf_outputs: &[env_defs::TfOutput],
) -> Result<(), anyhow::Error> {
    let mut errors = Vec::new();

    for output in tf_outputs {
        let original_name = &output.name;

        // Perform roundtrip: snake_case -> camelCase -> snake_case
        let camel_case = crate::to_camel_case(original_name);
        let back_to_snake = crate::to_snake_case(&camel_case);

        if original_name != &back_to_snake {
            errors.push(format!(
                "Output '{}' fails roundtrip case conversion: '{}' -> '{}' -> '{}'. \
                Outputs must use snake_case naming (e.g., 'my_output', 'bucket_arn') \
                to ensure proper conversion to camelCase for the API.",
                original_name, original_name, camel_case, back_to_snake
            ));
        }
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(anyhow::anyhow!(
            "Output name roundtrip verification failed:\n{}",
            errors.join("\n")
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use env_defs::{
        Metadata, ModuleManifest, ModuleSpec, ProviderManifest, ProviderMetaData, ProviderResp,
        ProviderSpec, TfOutput, TfVariable,
    };
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
            "tags": {
                "Name234": "my-s3bucket",
                "Environment43": "dev"
            }
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
            "tags": {
                "Name234": "my-s3bucket",
                "Environment43": "dev"
            }
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
            "tags": {
                "Name234": "my-s3bucket",
                "Environment43": "dev"
            }
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
                    providers: Vec::with_capacity(0),
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
            tf_providers: vec![],
            tf_required_providers: vec![],
            tf_lock_providers: vec![],
            stack_data: None,
            version_diff: None,
            cpu: "1024".to_string(),
            memory: "4096".to_string(),
            deprecated: false,
            deprecated_message: None,
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
                    providers: Vec::with_capacity(0),
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
            tf_providers: Vec::with_capacity(0),
            tf_required_providers: vec![],
            tf_lock_providers: vec![],
            stack_data: None,
            version_diff: None,
            cpu: "1024".to_string(),
            memory: "4096".to_string(),
            deprecated: false,
            deprecated_message: None,
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
                    providers: vec![env_defs::Provider {
                        name: "aws-5-default".to_string(),
                    }],
                },
            },
            tf_outputs: vec![
                TfOutput {
                    name: "bucket_arn".to_string(),
                    description: "ARN of the bucket".to_string(),
                    value: "".to_string(),
                    sensitive: None,
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
            tf_providers: vec![
                ProviderResp {
                    name:"aws-5-default".to_string(),
                    timestamp:"2024-10-10T22:23:14.368+02:00".to_string(),
                    description:"Some description...".to_string(),
                    reference:"https://github.com/infraweave-io/providers/aws-5-default".to_string(),
                    version:">= 5.81.0, < 6.0.0".to_string(),
                    s3_key:"s3bucket/providers/aws-5-default-5.81.0.zip".to_string(),
                    manifest: ProviderManifest {
                        metadata: ProviderMetaData {
                            name: "asw".to_string() 
                        },
                        api_version: "infraweave.io/v1".to_string(),
                        kind: "Provider".to_string(),
                        spec: ProviderSpec {
                            provider: "aws".to_string(), 
                            alias: None,
                            version: None,
                            description: "description".to_string(), 
                            reference: "reference".to_string() 
                        }
                    },
                    tf_variables: vec![
                        TfVariable {
                            default: None,
                            name: "tags".to_string(),
                            description: "Provider tags".to_string(),
                            _type: Value::String("map(string)".to_string()),
                            nullable: false,
                            sensitive: false,
                        }
                    ],
                    tf_extra_environment_variables: Vec::new(),
                }
            ],
            deprecated: false,
            deprecated_message: None,
        }
    }

    // Tests for verify_variable_name_roundtrip

    #[test]
    fn test_roundtrip_valid_snake_case() {
        // Valid snake_case variables that should pass roundtrip
        let variables = vec![
            TfVariable {
                name: "bucket_name".to_string(),
                description: "Test variable".to_string(),
                _type: Value::String("string".to_string()),
                default: None,
                nullable: false,
                sensitive: false,
            },
            TfVariable {
                name: "max_size".to_string(),
                description: "Test variable".to_string(),
                _type: Value::String("number".to_string()),
                default: None,
                nullable: false,
                sensitive: false,
            },
            TfVariable {
                name: "enable_logging".to_string(),
                description: "Test variable".to_string(),
                _type: Value::Bool(false),
                default: None,
                nullable: false,
                sensitive: false,
            },
        ];

        let result = verify_variable_name_roundtrip(&variables);
        assert!(result.is_ok(), "Valid snake_case variables should pass");
    }

    #[test]
    fn test_roundtrip_single_word() {
        // Single word variables (no underscores) should pass
        let variables = vec![
            TfVariable {
                name: "tags".to_string(),
                description: "Test variable".to_string(),
                _type: Value::String("map(string)".to_string()),
                default: None,
                nullable: false,
                sensitive: false,
            },
            TfVariable {
                name: "region".to_string(),
                description: "Test variable".to_string(),
                _type: Value::String("string".to_string()),
                default: None,
                nullable: false,
                sensitive: false,
            },
        ];

        let result = verify_variable_name_roundtrip(&variables);
        assert!(result.is_ok(), "Single word variables should pass");
    }

    #[test]
    fn test_roundtrip_with_numbers() {
        // Variables with numbers in the middle of words should pass
        let variables = vec![
            TfVariable {
                name: "http2_enabled".to_string(),
                description: "Test variable".to_string(),
                _type: Value::Bool(false),
                default: None,
                nullable: false,
                sensitive: false,
            },
            TfVariable {
                name: "bucket_v2".to_string(),
                description: "Test variable".to_string(),
                _type: Value::String("string".to_string()),
                default: None,
                nullable: false,
                sensitive: false,
            },
        ];

        let result = verify_variable_name_roundtrip(&variables);
        assert!(result.is_ok(), "Variables with numbers should pass");
    }

    #[test]
    fn test_roundtrip_number_after_underscore_fails() {
        // Variables like port_8080 fail because the number doesn't get capitalized
        // in camelCase, so port_8080 -> port8080 -> port8080 (doesn't match original)
        let variables = vec![TfVariable {
            name: "port_8080".to_string(),
            description: "Test variable".to_string(),
            _type: Value::String("number".to_string()),
            default: None,
            nullable: false,
            sensitive: false,
        }];

        let result = verify_variable_name_roundtrip(&variables);
        assert!(
            result.is_err(),
            "Variables with numbers immediately after underscore should fail"
        );
    }

    #[test]
    fn test_roundtrip_infraweave_prefix_skipped() {
        // INFRAWEAVE_ prefixed variables should be skipped
        let variables = vec![
            TfVariable {
                name: "INFRAWEAVE_REFERENCE".to_string(),
                description: "Test variable".to_string(),
                _type: Value::String("string".to_string()),
                default: None,
                nullable: false,
                sensitive: false,
            },
            TfVariable {
                name: "bucket_name".to_string(),
                description: "Test variable".to_string(),
                _type: Value::String("string".to_string()),
                default: None,
                nullable: false,
                sensitive: false,
            },
        ];

        let result = verify_variable_name_roundtrip(&variables);
        assert!(
            result.is_ok(),
            "INFRAWEAVE_ prefixed variables should be skipped"
        );
    }

    #[test]
    fn test_roundtrip_fail_camel_case() {
        // Variables in camelCase should fail (they won't roundtrip correctly)
        let variables = vec![TfVariable {
            name: "bucketName".to_string(),
            description: "Test variable".to_string(),
            _type: Value::String("string".to_string()),
            default: None,
            nullable: false,
            sensitive: false,
        }];

        let result = verify_variable_name_roundtrip(&variables);
        assert!(
            result.is_err(),
            "CamelCase variable names should fail roundtrip"
        );
        let error_msg = format!("{}", result.unwrap_err());
        assert!(
            error_msg.contains("bucketName"),
            "Error should mention the problematic variable"
        );
        assert!(
            error_msg.contains("roundtrip"),
            "Error should mention roundtrip"
        );
    }

    #[test]
    fn test_roundtrip_fail_double_underscore() {
        // Variables with double underscores should fail (they lose information)
        let variables = vec![TfVariable {
            name: "bucket__name".to_string(),
            description: "Test variable".to_string(),
            _type: Value::String("string".to_string()),
            default: None,
            nullable: false,
            sensitive: false,
        }];

        let result = verify_variable_name_roundtrip(&variables);
        assert!(
            result.is_err(),
            "Double underscore variable names should fail roundtrip"
        );
        let error_msg = format!("{}", result.unwrap_err());
        assert!(
            error_msg.contains("bucket__name"),
            "Error should mention the problematic variable"
        );
    }

    #[test]
    fn test_roundtrip_fail_pascal_case() {
        // Variables in PascalCase should fail
        let variables = vec![TfVariable {
            name: "BucketName".to_string(),
            description: "Test variable".to_string(),
            _type: Value::String("string".to_string()),
            default: None,
            nullable: false,
            sensitive: false,
        }];

        let result = verify_variable_name_roundtrip(&variables);
        assert!(
            result.is_err(),
            "PascalCase variable names should fail roundtrip"
        );
    }

    #[test]
    fn test_roundtrip_mixed_valid_and_invalid() {
        // Mix of valid and invalid variables - should fail and report all issues
        let variables = vec![
            TfVariable {
                name: "bucket_name".to_string(), // Valid
                description: "Test variable".to_string(),
                _type: Value::String("string".to_string()),
                default: None,
                nullable: false,
                sensitive: false,
            },
            TfVariable {
                name: "maxSize".to_string(), // Invalid - camelCase
                description: "Test variable".to_string(),
                _type: Value::String("number".to_string()),
                default: None,
                nullable: false,
                sensitive: false,
            },
            TfVariable {
                name: "enable_logging".to_string(), // Valid
                description: "Test variable".to_string(),
                _type: Value::Bool(false),
                default: None,
                nullable: false,
                sensitive: false,
            },
            TfVariable {
                name: "tag__value".to_string(), // Invalid - double underscore
                description: "Test variable".to_string(),
                _type: Value::String("string".to_string()),
                default: None,
                nullable: false,
                sensitive: false,
            },
        ];

        let result = verify_variable_name_roundtrip(&variables);
        assert!(
            result.is_err(),
            "Should fail when any variable fails roundtrip"
        );
        let error_msg = format!("{}", result.unwrap_err());
        assert!(
            error_msg.contains("maxSize"),
            "Error should mention maxSize"
        );
        assert!(
            error_msg.contains("tag__value"),
            "Error should mention tag__value"
        );
        assert!(
            !error_msg.contains("bucket_name"),
            "Error should not mention valid variables"
        );
    }

    #[test]
    fn test_roundtrip_empty_list() {
        // Empty list should pass
        let variables: Vec<TfVariable> = vec![];
        let result = verify_variable_name_roundtrip(&variables);
        assert!(result.is_ok(), "Empty variable list should pass");
    }

    // Tests for verify_output_name_roundtrip

    #[test]
    fn test_output_roundtrip_valid_snake_case() {
        // Valid snake_case outputs that should pass roundtrip
        let outputs = vec![
            TfOutput {
                name: "bucket_arn".to_string(),
                description: "Test output".to_string(),
                value: "".to_string(),
                sensitive: None,
            },
            TfOutput {
                name: "instance_id".to_string(),
                description: "Test output".to_string(),
                value: "".to_string(),
                sensitive: None,
            },
            TfOutput {
                name: "vpc_cidr_block".to_string(),
                description: "Test output".to_string(),
                value: "".to_string(),
                sensitive: None,
            },
        ];

        let result = verify_output_name_roundtrip(&outputs);
        assert!(result.is_ok(), "Valid snake_case outputs should pass");
    }

    #[test]
    fn test_output_roundtrip_single_word() {
        // Single word outputs (no underscores) should pass
        let outputs = vec![
            TfOutput {
                name: "arn".to_string(),
                description: "Test output".to_string(),
                value: "".to_string(),
                sensitive: None,
            },
            TfOutput {
                name: "id".to_string(),
                description: "Test output".to_string(),
                value: "".to_string(),
                sensitive: None,
            },
        ];

        let result = verify_output_name_roundtrip(&outputs);
        assert!(result.is_ok(), "Single word outputs should pass");
    }

    #[test]
    fn test_output_roundtrip_with_numbers() {
        // Outputs with numbers in the middle of words should pass
        let outputs = vec![
            TfOutput {
                name: "ipv4_address".to_string(),
                description: "Test output".to_string(),
                value: "".to_string(),
                sensitive: None,
            },
            TfOutput {
                name: "s3_bucket_arn".to_string(),
                description: "Test output".to_string(),
                value: "".to_string(),
                sensitive: None,
            },
        ];

        let result = verify_output_name_roundtrip(&outputs);
        assert!(result.is_ok(), "Outputs with numbers should pass");
    }

    #[test]
    fn test_output_roundtrip_number_after_underscore_fails() {
        // Outputs like port_8080 fail because the number doesn't get capitalized
        // in camelCase, so port_8080 -> port8080 -> port8080 (doesn't match original)
        let outputs = vec![TfOutput {
            name: "port_8080".to_string(),
            description: "Test output".to_string(),
            value: "".to_string(),
            sensitive: None,
        }];

        let result = verify_output_name_roundtrip(&outputs);
        assert!(
            result.is_err(),
            "Outputs with numbers immediately after underscore should fail"
        );
    }

    #[test]
    fn test_output_roundtrip_fail_camel_case() {
        // Outputs in camelCase should fail (they won't roundtrip correctly)
        let outputs = vec![TfOutput {
            name: "bucketArn".to_string(),
            description: "Test output".to_string(),
            value: "".to_string(),
            sensitive: None,
        }];

        let result = verify_output_name_roundtrip(&outputs);
        assert!(
            result.is_err(),
            "CamelCase output names should fail roundtrip"
        );
        let error_msg = format!("{}", result.unwrap_err());
        assert!(
            error_msg.contains("bucketArn"),
            "Error should mention the problematic output"
        );
        assert!(
            error_msg.contains("roundtrip"),
            "Error should mention roundtrip"
        );
    }

    #[test]
    fn test_output_roundtrip_fail_double_underscore() {
        // Outputs with double underscores should fail (they lose information)
        let outputs = vec![TfOutput {
            name: "bucket__arn".to_string(),
            description: "Test output".to_string(),
            value: "".to_string(),
            sensitive: None,
        }];

        let result = verify_output_name_roundtrip(&outputs);
        assert!(
            result.is_err(),
            "Double underscore output names should fail roundtrip"
        );
        let error_msg = format!("{}", result.unwrap_err());
        assert!(
            error_msg.contains("bucket__arn"),
            "Error should mention the problematic output"
        );
    }

    #[test]
    fn test_output_roundtrip_fail_pascal_case() {
        // Outputs in PascalCase should fail
        let outputs = vec![TfOutput {
            name: "BucketArn".to_string(),
            description: "Test output".to_string(),
            value: "".to_string(),
            sensitive: None,
        }];

        let result = verify_output_name_roundtrip(&outputs);
        assert!(
            result.is_err(),
            "PascalCase output names should fail roundtrip"
        );
    }

    #[test]
    fn test_output_roundtrip_mixed_valid_and_invalid() {
        // Mix of valid and invalid outputs - should fail and report all issues
        let outputs = vec![
            TfOutput {
                name: "bucket_arn".to_string(), // Valid
                description: "Test output".to_string(),
                value: "".to_string(),
                sensitive: None,
            },
            TfOutput {
                name: "instanceId".to_string(), // Invalid - camelCase
                description: "Test output".to_string(),
                value: "".to_string(),
                sensitive: None,
            },
            TfOutput {
                name: "vpc_id".to_string(), // Valid
                description: "Test output".to_string(),
                value: "".to_string(),
                sensitive: None,
            },
            TfOutput {
                name: "tag__value".to_string(), // Invalid - double underscore
                description: "Test output".to_string(),
                value: "".to_string(),
                sensitive: None,
            },
        ];

        let result = verify_output_name_roundtrip(&outputs);
        assert!(
            result.is_err(),
            "Should fail when any output fails roundtrip"
        );
        let error_msg = format!("{}", result.unwrap_err());
        assert!(
            error_msg.contains("instanceId"),
            "Error should mention instanceId"
        );
        assert!(
            error_msg.contains("tag__value"),
            "Error should mention tag__value"
        );
        assert!(
            !error_msg.contains("Output 'bucket_arn' fails"),
            "Error should not mention valid outputs"
        );
        assert!(
            !error_msg.contains("Output 'vpc_id' fails"),
            "Error should not mention valid outputs"
        );
    }

    #[test]
    fn test_output_roundtrip_empty_list() {
        // Empty list should pass
        let outputs: Vec<TfOutput> = vec![];
        let result = verify_output_name_roundtrip(&outputs);
        assert!(result.is_ok(), "Empty output list should pass");
    }
}
