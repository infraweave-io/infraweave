use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Resource mode indicating how Terraform manages the resource
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum ResourceMode {
    #[default]
    Managed,
    Data,
}

/// Action taken on a Terraform resource
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum ResourceAction {
    Create,
    Update,
    Delete,
    Replace,
    NoOp,
}

/// Sanitized resource change for audit trails.
/// Excludes sensitive values based on Terraform's sensitivity markers.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SanitizedResourceChange {
    /// Full Terraform resource address (e.g., "module.s3bucket.aws_s3_bucket.example")
    pub address: String,
    /// Resource type (e.g., "aws_s3_bucket")
    pub resource_type: String,
    /// Resource name (e.g., "example")
    pub name: String,
    /// Resource mode: managed or data
    pub mode: ResourceMode,
    /// Action taken on the resource
    pub action: ResourceAction,
    /// Resource attributes before change (sensitive values excluded)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub before: Option<Value>,
    /// Resource attributes after change (sensitive values excluded)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub after: Option<Value>,
}

impl SanitizedResourceChange {
    /// Parse resource change from Terraform plan JSON
    pub fn from_terraform_json(resource: &serde_json::Value) -> Option<Self> {
        let address = resource.get("address")?.as_str()?.to_string();
        let resource_type = resource.get("type")?.as_str()?.to_string();
        let name = resource.get("name")?.as_str()?.to_string();

        let mode = resource
            .get("mode")
            .and_then(|m| serde_json::from_value(m.clone()).ok())
            .unwrap_or_default();

        let change = resource.get("change")?;

        let actions: Vec<&str> = change
            .get("actions")?
            .as_array()?
            .iter()
            .filter_map(|a| a.as_str())
            .collect();

        let action = match actions.as_slice() {
            a if a.contains(&"delete") && a.contains(&"create") => ResourceAction::Replace,
            a if a.contains(&"delete") => ResourceAction::Delete,
            a if a.contains(&"create") => ResourceAction::Create,
            a if a.contains(&"update") => ResourceAction::Update,
            _ => ResourceAction::NoOp,
        };

        let before = if action != ResourceAction::Create {
            Self::sanitize_values(change.get("before"), change.get("before_sensitive"))
        } else {
            None
        };

        let after = if action != ResourceAction::Delete {
            Self::sanitize_values(change.get("after"), change.get("after_sensitive"))
        } else {
            None
        };

        Some(SanitizedResourceChange {
            address,
            resource_type,
            name,
            mode,
            action,
            before,
            after,
        })
    }

    /// Recursively filter out sensitive values using Terraform's sensitivity markers
    fn sanitize_values(values: Option<&Value>, sensitive_markers: Option<&Value>) -> Option<Value> {
        let values = values?;
        let sensitive_markers = sensitive_markers?;

        if sensitive_markers.as_bool() == Some(true) {
            return None;
        }

        match (values, sensitive_markers) {
            (Value::Object(val_map), Value::Object(sens_map)) => {
                let mut sanitized = serde_json::Map::new();

                for (key, val) in val_map {
                    if let Some(sens_val) = sens_map.get(key) {
                        if sens_val.as_bool() == Some(true) {
                            continue;
                        }
                        if let Some(sanitized_val) =
                            Self::sanitize_values(Some(val), Some(sens_val))
                        {
                            sanitized.insert(key.clone(), sanitized_val);
                        }
                    } else {
                        sanitized.insert(key.clone(), val.clone());
                    }
                }

                if sanitized.is_empty() {
                    None
                } else {
                    Some(Value::Object(sanitized))
                }
            }
            (Value::Array(val_arr), Value::Array(sens_arr)) => {
                let sanitized: Vec<Value> = val_arr
                    .iter()
                    .zip(sens_arr.iter())
                    .filter_map(|(val, sens)| Self::sanitize_values(Some(val), Some(sens)))
                    .collect();

                if sanitized.is_empty() {
                    None
                } else {
                    Some(Value::Array(sanitized))
                }
            }
            (val, Value::Bool(false)) | (val, Value::Object(_))
                if !val.is_object() && !val.is_array() =>
            {
                Some(val.clone())
            }
            _ => None,
        }
    }
}

/// Extract sanitized resource changes from full Terraform plan JSON
pub fn sanitize_resource_changes_from_plan(
    plan_json: &serde_json::Value,
) -> Vec<SanitizedResourceChange> {
    plan_json
        .get("resource_changes")
        .map(|resource_changes| sanitize_resource_changes(resource_changes))
        .unwrap_or_default()
}

/// Extract sanitized resource changes from resource_changes array
pub fn sanitize_resource_changes(
    resource_changes: &serde_json::Value,
) -> Vec<SanitizedResourceChange> {
    resource_changes
        .as_array()
        .map(|changes| {
            changes
                .iter()
                .filter_map(SanitizedResourceChange::from_terraform_json)
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_sanitize_no_op() {
        let resource_changes = json!([{
            "address": "module.s3bucket.aws_s3_bucket.example",
            "type": "aws_s3_bucket",
            "name": "example",
            "mode": "managed",
            "change": {
                "actions": ["no-op"],
                "before": {
                    "bucket": "my-bucket",
                    "tags": {"env": "prod"}
                },
                "after": {
                    "bucket": "my-bucket",
                    "tags": {"env": "prod"}
                },
                "before_sensitive": {},
                "after_sensitive": {}
            }
        }]);

        let sanitized = sanitize_resource_changes(&resource_changes);
        assert_eq!(sanitized.len(), 1);
        assert_eq!(
            sanitized[0].address,
            "module.s3bucket.aws_s3_bucket.example"
        );
        assert_eq!(sanitized[0].resource_type, "aws_s3_bucket");
        assert_eq!(sanitized[0].action, ResourceAction::NoOp);

        // Should include non-sensitive values
        assert!(sanitized[0].before.is_some());
        assert!(sanitized[0].after.is_some());
    }

    #[test]
    fn test_sanitize_replace() {
        let resource_changes = json!([{
            "address": "aws_instance.web",
            "type": "aws_instance",
            "name": "web",
            "mode": "managed",
            "change": {
                "actions": ["delete", "create"],
                "before": {
                    "instance_type": "t2.micro"
                },
                "after": {
                    "instance_type": "t3.micro"
                },
                "before_sensitive": {},
                "after_sensitive": {}
            }
        }]);

        let sanitized = sanitize_resource_changes(&resource_changes);
        assert_eq!(sanitized.len(), 1);
        assert_eq!(sanitized[0].action, ResourceAction::Replace);
        assert!(sanitized[0].before.is_some());
        assert!(sanitized[0].after.is_some());
    }

    #[test]
    fn test_sanitize_multiple_changes() {
        let resource_changes = json!([
            {
                "address": "aws_s3_bucket.new",
                "type": "aws_s3_bucket",
                "name": "new",
                "mode": "managed",
                "change": {
                    "actions": ["create"],
                    "after": {
                        "bucket": "new-bucket"
                    },
                    "after_sensitive": {}
                }
            },
            {
                "address": "aws_instance.old",
                "type": "aws_instance",
                "name": "old",
                "mode": "managed",
                "change": {
                    "actions": ["delete"],
                    "before": {
                        "instance_type": "t2.micro"
                    },
                    "before_sensitive": {}
                }
            }
        ]);

        let sanitized = sanitize_resource_changes(&resource_changes);
        assert_eq!(sanitized.len(), 2);
        assert_eq!(sanitized[0].action, ResourceAction::Create);
        assert!(sanitized[0].before.is_none()); // No before for create
        assert!(sanitized[0].after.is_some());

        assert_eq!(sanitized[1].action, ResourceAction::Delete);
        assert!(sanitized[1].before.is_some());
        assert!(sanitized[1].after.is_none()); // No after for delete
    }

    #[test]
    fn test_sanitize_with_sensitive_fields() {
        let resource_changes = json!([{
            "address": "aws_db_instance.example",
            "type": "aws_db_instance",
            "name": "example",
            "mode": "managed",
            "change": {
                "actions": ["create"],
                "after": {
                    "engine": "postgres",
                    "username": "admin",
                    "password": "super-secret-password",
                    "tags": {
                        "env": "prod",
                        "secret_tag": "secret-value"
                    }
                },
                "after_sensitive": {
                    "password": true,  // This should be excluded
                    "tags": {
                        "secret_tag": true  // This should be excluded
                    }
                }
            }
        }]);

        let sanitized = sanitize_resource_changes(&resource_changes);
        assert_eq!(sanitized.len(), 1);

        let after = sanitized[0].after.as_ref().unwrap();

        // Should include non-sensitive fields
        assert_eq!(after["engine"], "postgres");
        assert_eq!(after["username"], "admin");

        // Should exclude sensitive password
        assert!(after.get("password").is_none());

        // Should include tags object with only non-sensitive values
        let tags = after["tags"].as_object().unwrap();
        assert_eq!(tags["env"], "prod");
        assert!(tags.get("secret_tag").is_none());
    }

    #[test]
    fn test_sanitize_fully_sensitive_resource() {
        let resource_changes = json!([{
            "address": "aws_secretsmanager_secret.example",
            "type": "aws_secretsmanager_secret",
            "name": "example",
            "mode": "managed",
            "change": {
                "actions": ["create"],
                "after": {
                    "secret_string": "my-secret"
                },
                "after_sensitive": true  // Entire value is sensitive
            }
        }]);

        let sanitized = sanitize_resource_changes(&resource_changes);
        assert_eq!(sanitized.len(), 1);

        // When everything is sensitive, after should be None
        assert!(sanitized[0].after.is_none());
    }

    #[test]
    fn test_enum_serialization() {
        // Test serialization in struct context
        let change = SanitizedResourceChange {
            address: "aws_s3_bucket.test".to_string(),
            resource_type: "aws_s3_bucket".to_string(),
            name: "test".to_string(),
            mode: ResourceMode::Managed,
            action: ResourceAction::Create,
            before: None,
            after: Some(serde_json::json!({"bucket": "test"})),
        };

        let json = serde_json::to_value(&change).unwrap();

        // Verify enums serialize to lowercase strings
        assert_eq!(json["mode"], "managed");
        assert_eq!(json["action"], "create");

        // Verify deserialization works
        let deserialized: SanitizedResourceChange = serde_json::from_value(json).unwrap();
        assert_eq!(deserialized.mode, ResourceMode::Managed);
        assert_eq!(deserialized.action, ResourceAction::Create);

        // Test direct enum serialization
        assert_eq!(
            serde_json::to_value(ResourceMode::Managed).unwrap(),
            "managed"
        );
        assert_eq!(serde_json::to_value(ResourceMode::Data).unwrap(), "data");
        assert_eq!(
            serde_json::from_value::<ResourceMode>(serde_json::json!("managed")).unwrap(),
            ResourceMode::Managed
        );

        // Test ResourceAction
        assert_eq!(
            serde_json::to_value(ResourceAction::Create).unwrap(),
            "create"
        );
        assert_eq!(serde_json::to_value(ResourceAction::NoOp).unwrap(), "no-op");
        assert_eq!(
            serde_json::from_value::<ResourceAction>(serde_json::json!("no-op")).unwrap(),
            ResourceAction::NoOp
        );
    }

    #[test]
    fn test_data_mode_serialization() {
        let resource_changes = json!([{
            "address": "data.aws_ami.ubuntu",
            "type": "aws_ami",
            "name": "ubuntu",
            "mode": "data",
            "change": {
                "actions": ["no-op"],
                "before": {"id": "ami-123"},
                "after": {"id": "ami-123"},
                "before_sensitive": {},
                "after_sensitive": {}
            }
        }]);

        let sanitized = sanitize_resource_changes(&resource_changes);
        assert_eq!(sanitized.len(), 1);
        assert_eq!(sanitized[0].mode, ResourceMode::Data);

        // Verify it serializes back to "data"
        let json = serde_json::to_value(&sanitized[0]).unwrap();
        assert_eq!(json["mode"], "data");
    }

    #[test]
    fn test_sanitize_resource_changes_from_plan() {
        // Test with a complete Terraform plan JSON structure
        let plan_json = json!({
            "format_version": "1.2",
            "terraform_version": "1.5.0",
            "resource_changes": [
                {
                    "address": "aws_s3_bucket.example",
                    "type": "aws_s3_bucket",
                    "name": "example",
                    "mode": "managed",
                    "change": {
                        "actions": ["create"],
                        "after": {
                            "bucket": "my-new-bucket",
                            "tags": {"env": "prod"}
                        },
                        "after_sensitive": {}
                    }
                },
                {
                    "address": "data.aws_ami.ubuntu",
                    "type": "aws_ami",
                    "name": "ubuntu",
                    "mode": "data",
                    "change": {
                        "actions": ["no-op"],
                        "before": {"id": "ami-123"},
                        "after": {"id": "ami-123"},
                        "before_sensitive": {},
                        "after_sensitive": {}
                    }
                }
            ],
            "output_changes": {},
            "prior_state": {}
        });

        let sanitized = sanitize_resource_changes_from_plan(&plan_json);
        assert_eq!(sanitized.len(), 2);
        assert_eq!(sanitized[0].address, "aws_s3_bucket.example");
        assert_eq!(sanitized[0].action, ResourceAction::Create);
        assert_eq!(sanitized[1].address, "data.aws_ami.ubuntu");
        assert_eq!(sanitized[1].mode, ResourceMode::Data);
    }

    #[test]
    fn test_sanitize_resource_changes_from_plan_missing_field() {
        // Test with plan JSON that has no resource_changes field
        let plan_json = json!({
            "format_version": "1.2",
            "terraform_version": "1.5.0"
        });

        let sanitized = sanitize_resource_changes_from_plan(&plan_json);
        assert_eq!(sanitized.len(), 0);
    }

    #[test]
    fn test_sanitize_resource_changes_from_plan_empty_array() {
        // Test with plan JSON that has empty resource_changes
        let plan_json = json!({
            "format_version": "1.2",
            "terraform_version": "1.5.0",
            "resource_changes": []
        });

        let sanitized = sanitize_resource_changes_from_plan(&plan_json);
        assert_eq!(sanitized.len(), 0);
    }
}
