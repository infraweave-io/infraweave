use super::TfVariable;
use anyhow::anyhow;
use hcl::Block;

pub fn first_block(tf_code: &str) -> Result<Block, anyhow::Error> {
    let body = hcl::parse(tf_code)?;
    body.into_blocks()
        .next()
        .ok_or_else(|| anyhow!("HCL body has no blocks"))
}

mod required {
    use super::TfVariable;
    use serde_json::Value;

    #[test]
    fn required_when_default_is_none() {
        let v = TfVariable {
            name: "test".to_string(),
            nullable: true,
            ..Default::default()
        };
        assert_eq!(v.required(), true);
    }

    #[test]
    fn required_when_default_is_null_and_not_nullable() {
        let v = TfVariable {
            name: "test".to_string(),
            default: Some(Value::Null),
            nullable: false,
            ..Default::default()
        };
        assert_eq!(v.required(), true);
    }

    #[test]
    fn not_required_when_default_is_null_and_nullable() {
        let v = TfVariable {
            name: "test".to_string(),
            default: Some(Value::Null),
            nullable: true,
            ..Default::default()
        };
        assert_eq!(v.required(), false);
    }

    #[test]
    fn not_required_when_default_has_value() {
        let v = TfVariable {
            name: "test".to_string(),
            default: Some(Value::String("foo".to_string())),
            nullable: false,
            ..Default::default()
        };
        assert_eq!(v.required(), false);
    }
}

mod try_from {
    use super::TfVariable;
    use pretty_assertions::assert_eq;
    use serde_json::json;

    #[test]
    fn try_from_block_uses_defaults_and_overrides() {
        assert_eq!(
            TfVariable::try_from(
                &super::first_block(
                    r#"
variable "example" {
  type        = string
  description = "Example variable"
  default     = "value"
  nullable    = false
  sensitive   = true
}
"#
                )
                .unwrap()
            )
            .unwrap(),
            TfVariable {
                name: "example".to_string(),
                _type: serde_json::Value::String("string".to_string()),
                default: Some(serde_json::Value::String("value".to_string())),
                description: "Example variable".to_string(),
                nullable: false,
                sensitive: true,
            }
        );
    }

    #[test]
    fn fails_if_block_is_not_variable() {
        use std::convert::TryInto;

        let block = super::first_block(
            r#"
resource "example" {
  type = string
}
"#,
        )
        .unwrap();

        let result: Result<TfVariable, _> = (&block).try_into();
        let err = result.expect_err("expected error for non-variable block");
        assert_eq!(
            format!("{err}").contains("Block is not a variable block"),
            true
        );
    }

    #[test]
    fn fails_if_block_has_no_label() {
        use std::convert::TryInto;

        let block = super::first_block(
            r#"
variable {
  type = string
}
"#,
        )
        .unwrap();

        let result: Result<TfVariable, _> = (&block).try_into();
        let msg = format!("{}", result.expect_err("expected error for missing label"));
        assert_eq!(
            msg.contains("Variable block should have exactly one label"),
            true
        );
    }

    #[test]
    fn fails_if_block_has_multiple_labels_error_message() {
        use std::convert::TryInto;

        let block = super::first_block(
            r#"
variable "first" "second" {
  type = string
}
"#,
        )
        .unwrap();

        let result: Result<TfVariable, _> = (&block).try_into();
        let msg = format!(
            "{}",
            result.expect_err("expected error for multiple labels")
        );
        assert_eq!(
            msg.contains("Variable block should have exactly one label"),
            true
        );
    }

    #[test]
    fn fails_if_block_has_multiple_labels_mentions_both_labels() {
        use std::convert::TryInto;

        let block = super::first_block(
            r#"
variable "first" "second" {
  type = string
}
"#,
        )
        .unwrap();

        let result: Result<TfVariable, _> = (&block).try_into();
        let msg = format!(
            "{}",
            result.expect_err("expected error for multiple labels")
        );
        assert_eq!(msg.contains("first") && msg.contains("second"), true);
    }

    #[test]
    fn uses_defaults_when_only_name_is_set() {
        use std::convert::TryInto;

        let block = super::first_block(
            r#"
variable "only_name" {}
"#,
        )
        .unwrap();

        let var: TfVariable = (&block).try_into().unwrap();

        assert_eq!(
            var,
            TfVariable {
                name: "only_name".to_string(),
                ..Default::default()
            }
        );
    }

    #[test]
    fn ignores_unknown_attributes() {
        use std::convert::TryInto;

        let block = super::first_block(
            r#"
variable "example" {
  foo = "bar"
  bar = 42
}
"#,
        )
        .unwrap();

        let var: TfVariable = (&block).try_into().unwrap();

        assert_eq!(
            var,
            TfVariable {
                name: "example".to_string(),
                ..Default::default()
            }
        );
    }

    #[test]
    fn parses_default_number() {
        use std::convert::TryInto;

        let block = super::first_block(
            r#"
variable "num" {
  default = 42
}
"#,
        )
        .unwrap();
        let var: TfVariable = (&block).try_into().unwrap();
        assert_eq!(var.default, Some(json!(42)));
    }

    #[test]
    fn parses_default_bool() {
        use std::convert::TryInto;

        let block = super::first_block(
            r#"
variable "flag" {
  default = true
}
"#,
        )
        .unwrap();
        let var: TfVariable = (&block).try_into().unwrap();
        assert_eq!(var.default, Some(json!(true)));
    }

    #[test]
    fn parses_default_null() {
        use std::convert::TryInto;

        let block = super::first_block(
            r#"
variable "maybe" {
  default = null
}
"#,
        )
        .unwrap();
        let var: TfVariable = (&block).try_into().unwrap();
        assert_eq!(var.default, Some(serde_json::Value::Null));
    }

    #[test]
    fn sets_type_simple_string() {
        use std::convert::TryInto;

        let block = super::first_block(
            r#"
variable "simple" {
  type = string
}
"#,
        )
        .unwrap();
        let var: TfVariable = (&block).try_into().unwrap();
        assert_eq!(var._type, json!("string"));
    }

    #[test]
    fn sets_type_list_expression() {
        use std::convert::TryInto;

        let block = super::first_block(
            r#"
variable "list" {
  type = list(string)
}
"#,
        )
        .unwrap();
        let var: TfVariable = (&block).try_into().unwrap();
        assert_eq!(var._type, json!("list(string)"));
    }

    #[test]
    fn sets_type_map_string_no_default() {
        use std::convert::TryInto;

        let block = super::first_block(
            r#"
variable "tags" {
  type = map(string)
}
"#,
        )
        .unwrap();
        let var: TfVariable = (&block).try_into().unwrap();
        assert_eq!(var._type, json!("map(string)"));
        assert_eq!(var.default, None);
        assert_eq!(var.name, "tags");
    }

    #[test]
    fn parses_default_map_string() {
        use std::convert::TryInto;

        let block = super::first_block(
            r#"
variable "tags" {
  type = map(string)
  default = {
    "tag_environment" = "some_value1"
    "tag_name"        = "some_value2"
  }
}
"#,
        )
        .unwrap();
        let var: TfVariable = (&block).try_into().unwrap();
        assert_eq!(var._type, json!("map(string)"));
        assert_eq!(
            var.default,
            Some(json!({
                "tag_environment": "some_value1",
                "tag_name": "some_value2"
            }))
        );
    }

    #[test]
    fn sets_type_set_string_no_default() {
        use std::convert::TryInto;

        let block = super::first_block(
            r#"
variable "tags" {
  type = set(string)
}
"#,
        )
        .unwrap();
        let var: TfVariable = (&block).try_into().unwrap();
        assert_eq!(var._type, json!("set(string)"));
        assert_eq!(var.default, None);
        assert_eq!(var.name, "tags");
    }

    #[test]
    fn parses_default_set_string() {
        use std::convert::TryInto;

        let block = super::first_block(
            r#"
variable "items" {
  type    = set(string)
  default = ["a", "b"]
}
"#,
        )
        .unwrap();
        let var: TfVariable = (&block).try_into().unwrap();
        assert_eq!(var._type, json!("set(string)"));
        assert_eq!(var.default, Some(json!(["a", "b"])));
    }

    #[test]
    fn description_non_string_becomes_empty() {
        use std::convert::TryInto;

        let block = super::first_block(
            r#"
variable "example" {
  description = 123
}
"#,
        )
        .unwrap();

        let var: TfVariable = (&block).try_into().unwrap();
        assert_eq!(var.description, "");
    }

    #[test]
    fn fails_if_nullable_is_not_bool() {
        use std::convert::TryInto;

        let block = super::first_block(
            r#"
variable "example" {
  nullable = "false"
}
"#,
        )
        .unwrap();

        let result: Result<TfVariable, _> = (&block).try_into();
        let msg = format!(
            "{}",
            result.expect_err("expected error for non-bool nullable")
        );
        assert_eq!(msg.contains("nullable must be a bool"), true);
    }

    #[test]
    fn nullable_true_is_respected() {
        use std::convert::TryInto;

        let block = super::first_block(
            r#"
variable "example_true" {
  nullable = true
}
"#,
        )
        .unwrap();
        let var: TfVariable = (&block).try_into().unwrap();
        assert_eq!(var.nullable, true);
    }

    #[test]
    fn nullable_false_is_respected() {
        use std::convert::TryInto;

        let block = super::first_block(
            r#"
variable "example_false" {
  nullable = false
}
"#,
        )
        .unwrap();
        let var: TfVariable = (&block).try_into().unwrap();
        assert_eq!(var.nullable, false);
    }

    #[test]
    fn fails_if_sensitive_is_not_bool() {
        use std::convert::TryInto;

        let block = super::first_block(
            r#"
variable "example" {
  sensitive = "true"
}
"#,
        )
        .unwrap();

        let result: Result<TfVariable, _> = (&block).try_into();
        let msg = format!(
            "{}",
            result.expect_err("expected error for non-bool sensitive")
        );
        assert_eq!(msg.contains("sensitive must be a bool"), true);
    }

    #[test]
    fn sensitive_true_is_respected() {
        use std::convert::TryInto;

        let block = super::first_block(
            r#"
variable "example_true" {
  sensitive = true
}
"#,
        )
        .unwrap();
        let var: TfVariable = (&block).try_into().unwrap();
        assert_eq!(var.sensitive, true);
    }

    #[test]
    fn sensitive_false_is_respected() {
        use std::convert::TryInto;

        let block = super::first_block(
            r#"
variable "example_false" {
  sensitive = false
}
"#,
        )
        .unwrap();
        let var: TfVariable = (&block).try_into().unwrap();
        assert_eq!(var.sensitive, false);
    }
}

mod to_block {
    use super::TfVariable;
    use pretty_assertions::assert_eq;
    use std::convert::TryFrom;

    #[test]
    fn roundtrip_variable_block() {
        let tf_code = r#"
variable "example" {
  type        = string
  description = "Example variable"
  default     = "value"
  nullable    = false
  sensitive   = true
}
"#;
        let expected_block = super::first_block(tf_code).unwrap();
        let var = TfVariable::try_from(&expected_block).unwrap();
        let actual_block = var.to_block();
        assert_eq!(actual_block, expected_block);
    }

    #[test]
    fn to_block_without_default_omits_default() {
        let tf_code = r#"
variable "no_default" {
  type        = string
  description = "No default value"
  nullable    = true
  sensitive   = false
}
"#;
        let expected_block = super::first_block(tf_code).unwrap();
        let var = TfVariable::try_from(&expected_block).unwrap();
        let actual_block = var.to_block();
        assert_eq!(actual_block, expected_block);
        let tf = hcl::format::to_string(&actual_block).unwrap();
        assert!(
            !tf.contains("default ="),
            "expected no default attribute, got:\n{}",
            tf
        );
    }

    #[test]
    fn roundtrip_minimal_variable_only_name() {
        let tf_code = r#"
variable "minimal" {
  type        = string
  description = ""
  nullable    = true
  sensitive   = false
}
"#;
        let expected_block = super::first_block(tf_code).unwrap();
        let var = TfVariable::try_from(&expected_block).unwrap();
        let actual_block = var.to_block();
        assert_eq!(actual_block, expected_block);
    }

    #[test]
    fn roundtrip_variable_with_type_list_string() {
        let tf_code = r#"
variable "list_var" {
  type        = list(string)
  description = ""
  nullable    = true
  sensitive   = false
}
"#;
        let expected_block = super::first_block(tf_code).unwrap();
        let var = TfVariable::try_from(&expected_block).unwrap();
        let actual_block = var.to_block();
        assert_eq!(actual_block, expected_block);
    }

    #[test]
    fn roundtrip_variable_with_default_number() {
        let tf_code = r#"
variable "num" {
  type        = number
  description = ""
  default     = 42
  nullable    = true
  sensitive   = false
}
"#;
        let expected_block = super::first_block(tf_code).unwrap();
        let var = TfVariable::try_from(&expected_block).unwrap();
        let actual_block = var.to_block();
        assert_eq!(actual_block, expected_block);
    }

    #[test]
    fn roundtrip_variable_with_default_bool() {
        let tf_code = r#"
variable "flag" {
  type        = bool
  description = ""
  default     = true
  nullable    = true
  sensitive   = false
}
"#;
        let expected_block = super::first_block(tf_code).unwrap();
        let var = TfVariable::try_from(&expected_block).unwrap();
        let actual_block = var.to_block();
        assert_eq!(actual_block, expected_block);
    }

    #[test]
    fn roundtrip_variable_with_default_null() {
        let tf_code = r#"
variable "maybe" {
  type        = string
  description = ""
  default     = null
  nullable    = true
  sensitive   = false
}
"#;
        let expected_block = super::first_block(tf_code).unwrap();
        let var = TfVariable::try_from(&expected_block).unwrap();
        let actual_block = var.to_block();
        assert_eq!(actual_block, expected_block);
    }

    #[test]
    fn roundtrip_variable_with_default_string() {
        let tf_code = r#"
variable "with_str" {
  type        = string
  description = "A string default"
  default     = "hello"
  nullable    = true
  sensitive   = false
}
"#;
        let expected_block = super::first_block(tf_code).unwrap();
        let var = TfVariable::try_from(&expected_block).unwrap();
        let actual_block = var.to_block();
        assert_eq!(actual_block, expected_block);
    }

    #[test]
    fn roundtrip_variable_nullable_false_sensitive_true() {
        let tf_code = r#"
variable "secret" {
  type        = string
  description = "A secret"
  nullable    = false
  sensitive   = true
}
"#;
        let expected_block = super::first_block(tf_code).unwrap();
        let var = TfVariable::try_from(&expected_block).unwrap();
        let actual_block = var.to_block();
        assert_eq!(actual_block, expected_block);
    }

    #[test]
    fn roundtrip_variable_with_type_map_string() {
        let tf_code = r#"
variable "tags" {
  type        = map(string)
  description = ""
  nullable    = true
  sensitive   = false
}
"#;
        let expected_block = super::first_block(tf_code).unwrap();
        let var = TfVariable::try_from(&expected_block).unwrap();
        let actual_block = var.to_block();
        assert_eq!(actual_block, expected_block);
    }

    #[test]
    fn roundtrip_variable_with_type_map_string_and_default() {
        let tf_code = r#"
variable "tags" {
  type        = map(string)
  description = ""
  default     = {
    "tag_environment" = "some_value1"
    "tag_name"        = "some_value2"
  }
  nullable    = true
  sensitive   = false
}
"#;
        let expected_block = super::first_block(tf_code).unwrap();
        let var = TfVariable::try_from(&expected_block).unwrap();
        let actual_block = var.to_block();
        assert_eq!(actual_block, expected_block);
    }

    #[test]
    fn roundtrip_variable_with_type_set_string() {
        let tf_code = r#"
variable "items" {
  type        = set(string)
  description = ""
  nullable    = true
  sensitive   = false
}
"#;
        let expected_block = super::first_block(tf_code).unwrap();
        let var = TfVariable::try_from(&expected_block).unwrap();
        let actual_block = var.to_block();
        assert_eq!(actual_block, expected_block);
    }

    #[test]
    fn roundtrip_variable_with_type_set_string_and_default() {
        let tf_code = r#"
variable "items" {
  type        = set(string)
  description = ""
  default     = ["a", "b"]
  nullable    = true
  sensitive   = false
}
"#;
        let expected_block = super::first_block(tf_code).unwrap();
        let var = TfVariable::try_from(&expected_block).unwrap();
        let actual_block = var.to_block();
        assert_eq!(actual_block, expected_block);
    }
}
