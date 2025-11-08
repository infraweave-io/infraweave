use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::resource_change::SanitizedResourceChange;

pub fn get_change_record_identifier(
    project_id: &str,
    region: &str,
    deployment_id: &str,
    environment: &str,
) -> String {
    format!(
        "{}::{}::{}::{}",
        project_id, region, environment, deployment_id
    )
}

/// Represents changes in deployment variables
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct VariableChange {
    /// Variables that were added
    #[serde(skip_serializing_if = "Option::is_none")]
    pub added: Option<Value>,
    /// Variables that were removed
    #[serde(skip_serializing_if = "Option::is_none")]
    pub removed: Option<Value>,
    /// Variables that changed (before/after values)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub changed: Option<Value>,
    /// Variables that remained unchanged
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unchanged: Option<Value>,
}

impl VariableChange {
    /// Compute variable changes between before and after states
    pub fn compute(before: Option<&Value>, after: &Value) -> Option<Self> {
        let before_obj = before.and_then(|v| v.as_object());
        let after_obj = after.as_object()?;

        let mut added = serde_json::Map::new();
        let mut removed = serde_json::Map::new();
        let mut changed = serde_json::Map::new();
        let mut unchanged = serde_json::Map::new();

        // Find added and changed variables
        for (key, after_value) in after_obj {
            match before_obj.and_then(|b| b.get(key)) {
                Some(before_value) => {
                    if before_value != after_value {
                        // Variable changed - store both before and after
                        let mut change_entry = serde_json::Map::new();
                        change_entry.insert("before".to_string(), before_value.clone());
                        change_entry.insert("after".to_string(), after_value.clone());
                        changed.insert(key.clone(), Value::Object(change_entry));
                    } else {
                        // Variable unchanged
                        unchanged.insert(key.clone(), after_value.clone());
                    }
                }
                None => {
                    // Variable added
                    added.insert(key.clone(), after_value.clone());
                }
            }
        }

        // Find removed variables
        if let Some(before_obj) = before_obj {
            for (key, before_value) in before_obj {
                if !after_obj.contains_key(key) {
                    removed.insert(key.clone(), before_value.clone());
                }
            }
        }

        // Return None if no changes at all
        if added.is_empty() && removed.is_empty() && changed.is_empty() {
            return None;
        }

        Some(Self {
            added: if added.is_empty() {
                None
            } else {
                Some(Value::Object(added))
            },
            removed: if removed.is_empty() {
                None
            } else {
                Some(Value::Object(removed))
            },
            changed: if changed.is_empty() {
                None
            } else {
                Some(Value::Object(changed))
            },
            unchanged: if unchanged.is_empty() {
                None
            } else {
                Some(Value::Object(unchanged))
            },
        })
    }
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct InfraChangeRecord {
    pub deployment_id: String,
    pub project_id: String,
    pub region: String,
    pub job_id: String,
    pub module: String,
    pub environment: String,
    pub change_type: String, // plan or apply
    pub module_version_before: Option<String>,
    pub module_version: String,
    pub epoch: u128,
    pub timestamp: String,
    /// Human-readable terraform output (plan/apply/destroy stdout)
    pub plan_std_output: String,
    /// Storage key for raw Terraform plan JSON (from `terraform show -json planfile`).
    /// Always contains the plan, even for apply/destroy. Stored in blob storage for compliance.
    /// Use `resource_changes` for non-sensitive audit trails.
    pub plan_raw_json_key: String,
    /// Sanitized resource changes (addresses and actions only, no sensitive values).
    /// Optional for backward compatibility.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub resource_changes: Vec<SanitizedResourceChange>,
    /// Variable changes (added/removed/changed/unchanged)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub variable_changes: Option<VariableChange>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_variable_change_all_added() {
        let before = None;
        let after = json!({
            "instance_type": "t2.micro",
            "region": "us-west-2",
            "tags": {"env": "dev"}
        });

        let result = VariableChange::compute(before, &after).unwrap();

        assert!(result.added.is_some());
        assert!(result.removed.is_none());
        assert!(result.changed.is_none());
        assert!(result.unchanged.is_none());

        let added = result.added.unwrap();
        assert_eq!(added["instance_type"], "t2.micro");
        assert_eq!(added["region"], "us-west-2");
        assert_eq!(added["tags"]["env"], "dev");
    }

    #[test]
    fn test_variable_change_all_removed() {
        let before = json!({
            "instance_type": "t2.micro",
            "region": "us-west-2"
        });
        let after = json!({});

        let result = VariableChange::compute(Some(&before), &after).unwrap();

        assert!(result.added.is_none());
        assert!(result.removed.is_some());
        assert!(result.changed.is_none());
        assert!(result.unchanged.is_none());

        let removed = result.removed.unwrap();
        assert_eq!(removed["instance_type"], "t2.micro");
        assert_eq!(removed["region"], "us-west-2");
    }

    #[test]
    fn test_variable_change_simple_value_changed() {
        let before = json!({
            "instance_type": "t2.micro",
            "region": "us-west-2"
        });
        let after = json!({
            "instance_type": "t2.small",
            "region": "us-west-2"
        });

        let result = VariableChange::compute(Some(&before), &after).unwrap();

        assert!(result.added.is_none());
        assert!(result.removed.is_none());
        assert!(result.changed.is_some());
        assert!(result.unchanged.is_some());

        let changed = result.changed.unwrap();
        assert_eq!(changed["instance_type"]["before"], "t2.micro");
        assert_eq!(changed["instance_type"]["after"], "t2.small");

        let unchanged = result.unchanged.unwrap();
        assert_eq!(unchanged["region"], "us-west-2");
    }

    #[test]
    fn test_variable_change_complex_nested_structure() {
        let before = json!({
            "vpc_config": {
                "cidr": "10.0.0.0/16",
                "subnets": ["10.0.1.0/24", "10.0.2.0/24"]
            },
            "tags": {
                "env": "dev",
                "team": "platform"
            }
        });
        let after = json!({
            "vpc_config": {
                "cidr": "10.0.0.0/16",
                "subnets": ["10.0.1.0/24", "10.0.2.0/24", "10.0.3.0/24"]
            },
            "tags": {
                "env": "prod",
                "team": "platform"
            }
        });

        let result = VariableChange::compute(Some(&before), &after).unwrap();

        assert!(result.added.is_none());
        assert!(result.removed.is_none());
        assert!(result.changed.is_some());
        assert!(result.unchanged.is_none());

        let changed = result.changed.unwrap();

        // vpc_config changed (different subnet array)
        assert_eq!(
            changed["vpc_config"]["before"]["subnets"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
        assert_eq!(
            changed["vpc_config"]["after"]["subnets"]
                .as_array()
                .unwrap()
                .len(),
            3
        );

        // tags changed (env changed from dev to prod)
        assert_eq!(changed["tags"]["before"]["env"], "dev");
        assert_eq!(changed["tags"]["after"]["env"], "prod");
    }

    #[test]
    fn test_variable_change_mixed_operations() {
        let before = json!({
            "instance_type": "t2.micro",
            "region": "us-west-2",
            "old_setting": "value1"
        });
        let after = json!({
            "instance_type": "t2.small",
            "region": "us-west-2",
            "new_setting": "value2"
        });

        let result = VariableChange::compute(Some(&before), &after).unwrap();

        assert!(result.added.is_some());
        assert!(result.removed.is_some());
        assert!(result.changed.is_some());
        assert!(result.unchanged.is_some());

        let added = result.added.unwrap();
        assert_eq!(added["new_setting"], "value2");

        let removed = result.removed.unwrap();
        assert_eq!(removed["old_setting"], "value1");

        let changed = result.changed.unwrap();
        assert_eq!(changed["instance_type"]["before"], "t2.micro");
        assert_eq!(changed["instance_type"]["after"], "t2.small");

        let unchanged = result.unchanged.unwrap();
        assert_eq!(unchanged["region"], "us-west-2");
    }

    #[test]
    fn test_variable_change_no_changes() {
        let before = json!({
            "instance_type": "t2.micro",
            "region": "us-west-2"
        });
        let after = json!({
            "instance_type": "t2.micro",
            "region": "us-west-2"
        });

        let result = VariableChange::compute(Some(&before), &after);

        // Should return None when no changes
        assert!(result.is_none());
    }

    #[test]
    fn test_variable_change_empty_objects() {
        let before = json!({});
        let after = json!({});

        let result = VariableChange::compute(Some(&before), &after);

        // Should return None when both are empty
        assert!(result.is_none());
    }

    #[test]
    fn test_variable_change_with_null_values() {
        let before = json!({
            "setting1": null,
            "setting2": "value"
        });
        let after = json!({
            "setting1": "not-null",
            "setting2": null
        });

        let result = VariableChange::compute(Some(&before), &after).unwrap();

        let changed = result.changed.unwrap();
        assert_eq!(changed["setting1"]["before"], Value::Null);
        assert_eq!(changed["setting1"]["after"], "not-null");
        assert_eq!(changed["setting2"]["before"], "value");
        assert_eq!(changed["setting2"]["after"], Value::Null);
    }

    #[test]
    fn test_variable_change_array_values() {
        let before = json!({
            "availability_zones": ["us-west-2a", "us-west-2b"]
        });
        let after = json!({
            "availability_zones": ["us-west-2a", "us-west-2b", "us-west-2c"]
        });

        let result = VariableChange::compute(Some(&before), &after).unwrap();

        let changed = result.changed.unwrap();
        assert_eq!(
            changed["availability_zones"]["before"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
        assert_eq!(
            changed["availability_zones"]["after"]
                .as_array()
                .unwrap()
                .len(),
            3
        );
    }

    #[test]
    fn test_variable_change_serialization() {
        let before = json!({
            "old_var": "old_value"
        });
        let after = json!({
            "new_var": "new_value"
        });

        let result = VariableChange::compute(Some(&before), &after).unwrap();

        // Test serialization
        let serialized = serde_json::to_string(&result).unwrap();
        assert!(serialized.contains("added"));
        assert!(serialized.contains("removed"));
        assert!(!serialized.contains("changed")); // Should be skipped as it's None
    }

    #[test]
    fn test_variable_change_only_unchanged_returns_none() {
        let before = json!({
            "var1": "value1",
            "var2": "value2"
        });
        let after = json!({
            "var1": "value1",
            "var2": "value2"
        });

        let result = VariableChange::compute(Some(&before), &after);

        // Returns None when only unchanged variables exist
        assert!(result.is_none());
    }
}
