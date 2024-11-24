use jsonschema::{Draft, JSONSchema};
use serde_json;
use serde_yaml;
use std::error::Error;

pub fn validate_module_schema(module_yaml: &String) -> Result<(), Box<dyn Error>> {
    let schema_yaml_value: serde_yaml::Value = serde_yaml::from_str(MODULE_SCHEMA_MANIFEST)?;
    let input_manifest: serde_yaml::Value =
        serde_yaml::from_str(&module_yaml).expect("Could not parse module yaml");
    return validate_schema(input_manifest, schema_yaml_value);
}

pub fn validate_policy_schema(module_yaml: &String) -> Result<(), Box<dyn Error>> {
    let schema_yaml_value: serde_yaml::Value = serde_yaml::from_str(POLICY_SCHEMA_MANIFEST)?;
    let input_manifest: serde_yaml::Value =
        serde_yaml::from_str(&module_yaml).expect("Could not parse policy yaml");
    return validate_schema(input_manifest, schema_yaml_value);
}

pub fn validate_schema(
    input_manifest: serde_yaml::Value,
    schema: serde_yaml::Value,
) -> Result<(), Box<dyn Error>> {
    let schema_json_value = serde_json::to_value(&schema)?;

    let compiled_schema = JSONSchema::options()
        .with_draft(Draft::Draft7)
        .compile(&schema_json_value)
        .expect("Invalid JSON Schema");

    let manifest_json_value = serde_json::to_value(&input_manifest)?;

    let result = compiled_schema.validate(&manifest_json_value);

    match result {
        Ok(_) => {
            println!("Schema validation succeeded");
            Ok(())
        }
        Err(errors) => {
            for error in errors {
                println!("Schema validation error: {}", error);
            }
            Err("Schema validation failed".into())
        }
    }
}

const MODULE_SCHEMA_MANIFEST: &str = r#"
type: object
properties:
  apiVersion:
    type: string
  kind:
    type: string
  metadata:
    type: object
    properties:
      name:
        type: string
    required:
      - name
  spec:
    type: object
    properties:
      moduleName:
        type: string
      version:
        type: string
      description:
        type: string
      reference:
        type: string
    required:
      - moduleName
      - description
      - reference
required:
  - apiVersion
  - kind
  - metadata
  - spec
"#;

const POLICY_SCHEMA_MANIFEST: &str = r#"
type: object
properties:
  apiVersion:
    type: string
  kind:
    type: string
  metadata:
    type: object
    properties:
      name:
        type: string
    required:
      - name
  spec:
    type: object
    properties:
      policyName:
        type: string
      version:
        type: string
      description:
        type: string
      reference:
        type: string
      data:
        type: object
    required:
      - policyName
      - version
      - description
      - reference
      - data
required:
  - apiVersion
  - kind
  - metadata
  - spec
"#;
