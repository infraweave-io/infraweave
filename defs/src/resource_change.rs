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

/// Represents changes in resource dependencies
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DependencyChange {
    /// Dependencies that were added
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub added: Vec<String>,
    /// Dependencies that were removed
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub removed: Vec<String>,
    /// Dependencies that remained unchanged
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub unchanged: Vec<String>,
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
    /// Provider source (e.g., "registry.terraform.io/hashicorp/aws")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    /// Action taken on the resource
    pub action: ResourceAction,
    /// Reason why this action is required (e.g., resource attributes require replacement)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub action_reason: Option<String>,
    /// Resource index for count/for_each resources (e.g., 0, "key", etc.)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub index: Option<Value>,
    /// Dependency changes (added/removed/unchanged)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub depends_on: Option<DependencyChange>,
    /// Resource attributes before change (only for create/delete actions)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub before: Option<Value>,
    /// Resource attributes after change (only for create/delete actions)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub after: Option<Value>,
    /// Compact representation of what changed (only for update/replace actions)
    /// Maps attribute paths to {"before": value, "after": value, "after_unknown": bool}
    /// Use null for additions (before: null) or deletions (after: null)
    /// after_unknown indicates if the after value is "known after apply"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub changes: Option<serde_json::Map<String, Value>>,
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

        let provider = resource
            .get("provider_name")
            .and_then(|p| p.as_str())
            .map(|s| s.to_string());

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

        // Extract action_reason if present
        let action_reason = resource
            .get("action_reason")
            .and_then(|r| r.as_str())
            .map(|s| s.to_string());

        // Extract index if present (for count/for_each resources)
        let index = resource.get("index").cloned();

        // Extract dependency changes
        let depends_on = Self::compute_dependency_change(
            change.get("before_depends_on"),
            change.get("after_depends_on"),
        );

        // For update/replace: only store changes, not full state
        // For create/delete: store full state (sanitized)
        // For no-op: store nothing (resource unchanged)
        let (before, after, changes) = match action {
            ResourceAction::Update | ResourceAction::Replace => {
                let changes = Self::compute_changes_with_sensitivity(
                    change.get("before"),
                    change.get("after"),
                    change.get("before_sensitive"),
                    change.get("after_sensitive"),
                    change.get("after_unknown"),
                );
                (None, None, changes)
            }
            ResourceAction::Create => {
                let after_sanitized =
                    Self::sanitize_values(change.get("after"), change.get("after_sensitive"));
                (None, after_sanitized, None)
            }
            ResourceAction::Delete => {
                let before_sanitized =
                    Self::sanitize_values(change.get("before"), change.get("before_sensitive"));
                (before_sanitized, None, None)
            }
            ResourceAction::NoOp => {
                // No state stored for no-op actions
                (None, None, None)
            }
        };

        Some(SanitizedResourceChange {
            address,
            resource_type,
            name,
            mode,
            provider,
            action,
            action_reason,
            index,
            depends_on,
            before,
            after,
            changes,
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

    /// Check if a value is marked as sensitive
    fn is_sensitive(sensitive_markers: Option<&Value>) -> bool {
        matches!(sensitive_markers, Some(Value::Bool(true)))
    }

    /// Compute dependency changes between before and after states
    fn compute_dependency_change(
        before_depends_on: Option<&Value>,
        after_depends_on: Option<&Value>,
    ) -> Option<DependencyChange> {
        let before_deps: Vec<String> = before_depends_on
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default();

        let after_deps: Vec<String> = after_depends_on
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default();

        if before_deps.is_empty() && after_deps.is_empty() {
            return None;
        }

        let before_set: std::collections::HashSet<_> = before_deps.iter().collect();
        let after_set: std::collections::HashSet<_> = after_deps.iter().collect();

        let added: Vec<String> = after_deps
            .iter()
            .filter(|dep| !before_set.contains(dep))
            .cloned()
            .collect();

        let removed: Vec<String> = before_deps
            .iter()
            .filter(|dep| !after_set.contains(dep))
            .cloned()
            .collect();

        let unchanged: Vec<String> = after_deps
            .iter()
            .filter(|dep| before_set.contains(dep))
            .cloned()
            .collect();

        if added.is_empty() && removed.is_empty() && unchanged.is_empty() {
            return None;
        }

        Some(DependencyChange {
            added,
            removed,
            unchanged,
        })
    }

    /// Compute compact diff between before and after values, marking sensitive changes as redacted
    /// and unknown values with after_unknown flag
    fn compute_changes_with_sensitivity(
        before: Option<&Value>,
        after: Option<&Value>,
        before_sensitive: Option<&Value>,
        after_sensitive: Option<&Value>,
        after_unknown: Option<&Value>,
    ) -> Option<serde_json::Map<String, Value>> {
        let (before_val, after_val) = (before?, after?);
        let mut changes = serde_json::Map::new();
        Self::diff_values_with_sensitivity(
            "",
            before_val,
            after_val,
            before_sensitive,
            after_sensitive,
            after_unknown,
            &mut changes,
        );

        if changes.is_empty() {
            None
        } else {
            Some(changes)
        }
    }

    /// Recursively diff two JSON values and record changes, including redacted sensitive changes
    /// and marking unknown values
    fn diff_values_with_sensitivity(
        path: &str,
        before: &Value,
        after: &Value,
        before_sensitive: Option<&Value>,
        after_sensitive: Option<&Value>,
        after_unknown: Option<&Value>,
        changes: &mut serde_json::Map<String, Value>,
    ) {
        // Check if this field is sensitive
        let is_sensitive =
            Self::is_sensitive(before_sensitive) || Self::is_sensitive(after_sensitive);

        // Check if this field is unknown
        let is_unknown = Self::is_sensitive(after_unknown);

        match (before, after) {
            (Value::Object(before_map), Value::Object(after_map)) => {
                // Get sensitivity and unknown maps if they exist
                let before_sens_map = before_sensitive.and_then(|v| v.as_object());
                let after_sens_map = after_sensitive.and_then(|v| v.as_object());
                let after_unknown_map = after_unknown.and_then(|v| v.as_object());

                // Check all keys in both maps
                let all_keys: std::collections::HashSet<_> =
                    before_map.keys().chain(after_map.keys()).collect();

                for key in all_keys {
                    let new_path = if path.is_empty() {
                        key.to_string()
                    } else {
                        format!("{}.{}", path, key)
                    };

                    let key_before_sens = before_sens_map.and_then(|m| m.get(key.as_str()));
                    let key_after_sens = after_sens_map.and_then(|m| m.get(key.as_str()));
                    let key_after_unknown = after_unknown_map.and_then(|m| m.get(key.as_str()));

                    match (before_map.get(key), after_map.get(key)) {
                        (Some(before_val), Some(after_val)) if before_val != after_val => {
                            Self::diff_values_with_sensitivity(
                                &new_path,
                                before_val,
                                after_val,
                                key_before_sens,
                                key_after_sens,
                                key_after_unknown,
                                changes,
                            );
                        }
                        (Some(before_val), None) => {
                            // Key removed
                            changes.insert(
                                new_path,
                                if Self::is_sensitive(key_before_sens) {
                                    serde_json::json!({
                                        "before": "[REDACTED]",
                                        "after": null,
                                        "after_unknown": false
                                    })
                                } else {
                                    serde_json::json!({
                                        "before": before_val,
                                        "after": null,
                                        "after_unknown": false
                                    })
                                },
                            );
                        }
                        (None, Some(after_val)) => {
                            // Key added
                            let is_unknown = Self::is_sensitive(key_after_unknown);
                            changes.insert(
                                new_path,
                                if Self::is_sensitive(key_after_sens) {
                                    serde_json::json!({
                                        "before": null,
                                        "after": "[REDACTED]",
                                        "after_unknown": is_unknown
                                    })
                                } else {
                                    serde_json::json!({
                                        "before": null,
                                        "after": after_val,
                                        "after_unknown": is_unknown
                                    })
                                },
                            );
                        }
                        _ => {}
                    }
                }
            }
            (Value::Array(before_arr), Value::Array(after_arr)) if before_arr != after_arr => {
                // For arrays, store the entire array change
                if is_sensitive {
                    changes.insert(
                        path.to_string(),
                        serde_json::json!({
                            "before": "[REDACTED]",
                            "after": "[REDACTED]",
                            "after_unknown": is_unknown
                        }),
                    );
                } else {
                    changes.insert(
                        path.to_string(),
                        serde_json::json!({
                            "before": before_arr,
                            "after": after_arr,
                            "after_unknown": is_unknown
                        }),
                    );
                }
            }
            _ if before != after => {
                // Primitive values that differ
                let (before_value, after_value) = if is_sensitive {
                    (
                        serde_json::json!("[REDACTED]"),
                        serde_json::json!("[REDACTED]"),
                    )
                } else {
                    (before.clone(), after.clone())
                };
                changes.insert(
                    path.to_string(),
                    serde_json::json!({
                        "before": before_value,
                        "after": after_value,
                        "after_unknown": is_unknown
                    }),
                );
            }
            _ => {}
        }
    }
}

/// Extract sanitized resource changes from full Terraform plan JSON
pub fn sanitize_resource_changes_from_plan(
    plan_json: &serde_json::Value,
) -> Vec<SanitizedResourceChange> {
    plan_json
        .get("resource_changes")
        .map(sanitize_resource_changes)
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

/// Format a JSON value for display in change output
fn format_value(value: &Value) -> String {
    match value {
        Value::String(s) => format!("\"{}\"", s),
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Null => "null".to_string(),
        Value::Array(arr) => {
            if arr.is_empty() {
                "[]".to_string()
            } else if arr.len() <= 3 {
                let items: Vec<String> = arr.iter().map(format_value).collect();
                format!("[{}]", items.join(", "))
            } else {
                format!("[... {} items ...]", arr.len())
            }
        }
        Value::Object(obj) => {
            if obj.is_empty() {
                "{}".to_string()
            } else {
                format!("{{... {} fields ...}}", obj.len())
            }
        }
    }
}

/// Format a value for change display, using after_unknown to detect "known after apply" values
fn format_change_value(value: Option<&Value>, is_after_unknown: bool) -> String {
    match value {
        Some(v) if !v.is_null() => format_value(v),
        Some(Value::Null) | None => {
            if is_after_unknown {
                // Value is marked as unknown in Terraform plan
                "(known after apply)".to_string()
            } else {
                // Actual null value
                "null".to_string()
            }
        }
        _ => "null".to_string(),
    }
}

/// Helper to extract and format changes from a changes_map
fn format_changes_from_map(
    changes_map: &serde_json::Map<String, Value>,
) -> (Vec<String>, Vec<&str>) {
    let mut concrete_changes = Vec::new();
    let mut known_after_apply = Vec::new();

    for (key, value) in changes_map.iter() {
        if let Some(obj) = value.as_object() {
            let is_unknown = obj
                .get("after_unknown")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);

            if is_unknown {
                known_after_apply.push(key.as_str());
            } else {
                let before_val = obj.get("before");
                let after_val = obj.get("after");
                let before = format_change_value(before_val, false);
                let after = format_change_value(after_val, is_unknown);
                concrete_changes.push(format!("{}: {} -> {}", key, before, after));
            }
        }
    }

    (concrete_changes, known_after_apply)
}

pub fn pretty_print_resource_changes(changes: &[SanitizedResourceChange]) -> String {
    if changes.is_empty() {
        return "No resource changes.".to_string();
    }

    let mut output = String::new();
    output.push_str(&format!("Total changes: {}\n\n", changes.len()));

    // Group by action
    let mut creates = Vec::new();
    let mut updates = Vec::new();
    let mut deletes = Vec::new();
    let mut replaces = Vec::new();
    let mut no_ops = Vec::new();

    for change in changes {
        match change.action {
            ResourceAction::Create => creates.push(change),
            ResourceAction::Update => updates.push(change),
            ResourceAction::Delete => deletes.push(change),
            ResourceAction::Replace => replaces.push(change),
            ResourceAction::NoOp => no_ops.push(change),
        }
    }

    // Print summary
    output.push_str("Summary:\n");
    if !creates.is_empty() {
        output.push_str(&format!("  + Create: {}\n", creates.len()));
    }
    if !updates.is_empty() {
        output.push_str(&format!("  ~ Update: {}\n", updates.len()));
    }
    if !replaces.is_empty() {
        output.push_str(&format!("  +/- Replace: {}\n", replaces.len()));
    }
    if !deletes.is_empty() {
        output.push_str(&format!("  - Delete: {}\n", deletes.len()));
    }
    if !no_ops.is_empty() {
        output.push_str(&format!("  = No-op: {}\n", no_ops.len()));
    }
    output.push('\n');

    // Print detailed changes
    if !creates.is_empty() {
        output.push_str("Resources to create:\n");
        for change in creates {
            output.push_str(&format!(
                "  + {} ({})\n",
                change.address, change.resource_type
            ));
        }
        output.push('\n');
    }

    if !updates.is_empty() {
        output.push_str("Resources to update:\n");
        for change in updates {
            output.push_str(&format!(
                "  ~ {} ({})\n",
                change.address, change.resource_type
            ));
            if let Some(ref changes_map) = change.changes {
                let (concrete_changes, known_after_apply) = format_changes_from_map(changes_map);

                // Show concrete changes first
                for change_str in concrete_changes {
                    output.push_str(&format!("      ~ {}\n", change_str));
                }

                // Group known-after-apply changes
                if !known_after_apply.is_empty() {
                    if known_after_apply.len() <= 5 {
                        output.push_str(&format!(
                            "      (known after apply: {})\n",
                            known_after_apply.join(", ")
                        ));
                    } else {
                        output.push_str(&format!(
                            "      (known after apply: {}, ... and {} more)\n",
                            known_after_apply[..5].join(", "),
                            known_after_apply.len() - 5
                        ));
                    }
                }
            }
        }
        output.push('\n');
    }

    if !replaces.is_empty() {
        output.push_str("Resources to replace:\n");
        for change in replaces {
            output.push_str(&format!(
                "  +/- {} ({})\n",
                change.address, change.resource_type
            ));
            if let Some(ref reason) = change.action_reason {
                output.push_str(&format!("      Reason: {}\n", reason));
            }
            // Show what's changing for replacements
            if let Some(ref changes_map) = change.changes {
                let (concrete_changes, known_after_apply) = format_changes_from_map(changes_map);

                // Show concrete changes first
                for change_str in concrete_changes {
                    output.push_str(&format!("      ~ {}\n", change_str));
                }

                // Group known-after-apply changes
                if !known_after_apply.is_empty() {
                    if known_after_apply.len() <= 5 {
                        output.push_str(&format!(
                            "      (known after apply: {})\n",
                            known_after_apply.join(", ")
                        ));
                    } else {
                        output.push_str(&format!(
                            "      (known after apply: {}, ... and {} more)\n",
                            known_after_apply[..5].join(", "),
                            known_after_apply.len() - 5
                        ));
                    }
                }
            }
        }
        output.push('\n');
    }

    if !deletes.is_empty() {
        output.push_str("Resources to delete:\n");
        for change in deletes {
            output.push_str(&format!(
                "  - {} ({})\n",
                change.address, change.resource_type
            ));
        }
        output.push('\n');
    }

    if !no_ops.is_empty() {
        output.push_str("Resources with no changes:\n");
        for change in no_ops {
            output.push_str(&format!(
                "  = {} ({})\n",
                change.address, change.resource_type
            ));
        }
        output.push('\n');
    }

    output
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

        // For no-op, we don't store any state (nothing changed)
        assert!(sanitized[0].before.is_none());
        assert!(sanitized[0].after.is_none());
        assert!(sanitized[0].changes.is_none());
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

        // For replace, we only store changes, not full before/after
        assert!(sanitized[0].before.is_none());
        assert!(sanitized[0].after.is_none());
        assert!(sanitized[0].changes.is_some());

        let changes = sanitized[0].changes.as_ref().unwrap();
        assert_eq!(
            changes.get("instance_type").unwrap(),
            &serde_json::json!({
                "before": "t2.micro",
                "after": "t3.micro",
                "after_unknown": false
            })
        );
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
        assert!(sanitized[0].before.is_none());
        assert!(sanitized[0].after.is_some());
        assert!(sanitized[0].changes.is_none());

        assert_eq!(sanitized[1].action, ResourceAction::Delete);
        assert!(sanitized[1].before.is_some());
        assert!(sanitized[1].after.is_none());
        assert!(sanitized[1].changes.is_none());
    }

    #[test]
    fn test_sanitize_with_sensitive_fields() {
        let resource_changes = json!([{
            "address": "aws_db_instance.example",
            "type": "aws_db_instance",
            "name": "example",
            "mode": "managed",
            "change": {
                "actions": ["update"],
                "before": {
                    "engine": "postgres",
                    "username": "admin",
                    "password": "old-password",
                    "db_name": "mydb",
                    "tags": {
                        "env": "prod",
                        "secret_tag": "secret-value"
                    }
                },
                "after": {
                    "engine": "postgres",
                    "username": "newadmin",
                    "password": "super-secret-password",
                    "db_name": "mydb",
                    "tags": {
                        "env": "staging",
                        "secret_tag": "new-secret-value"
                    }
                },
                "before_sensitive": {
                    "password": true,
                    "tags": {
                        "secret_tag": true
                    }
                },
                "after_sensitive": {
                    "password": true,
                    "tags": {
                        "secret_tag": true
                    }
                }
            }
        }]);

        let sanitized = sanitize_resource_changes(&resource_changes);
        assert_eq!(sanitized.len(), 1);

        // For updates, we only have changes, not full before/after
        assert!(sanitized[0].before.is_none());
        assert!(sanitized[0].after.is_none());
        assert!(sanitized[0].changes.is_some());

        let changes = sanitized[0].changes.as_ref().unwrap();

        // Sensitive password change should be shown as [REDACTED]
        assert_eq!(
            changes.get("password").unwrap(),
            &serde_json::json!({
                "before": "[REDACTED]",
                "after": "[REDACTED]",
                "after_unknown": false
            })
        );

        // Sensitive tag change should be shown as [REDACTED]
        assert_eq!(
            changes.get("tags.secret_tag").unwrap(),
            &serde_json::json!({
                "before": "[REDACTED]",
                "after": "[REDACTED]",
                "after_unknown": false
            })
        );

        // Non-sensitive changes should show actual values
        assert_eq!(
            changes.get("username").unwrap(),
            &serde_json::json!({
                "before": "admin",
                "after": "newadmin",
                "after_unknown": false
            })
        );

        assert_eq!(
            changes.get("tags.env").unwrap(),
            &serde_json::json!({
                "before": "prod",
                "after": "staging",
                "after_unknown": false
            })
        );

        // Unchanged fields shouldn't appear
        assert!(changes.get("engine").is_none());
        assert!(changes.get("db_name").is_none());
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
            provider: None,
            action: ResourceAction::Create,
            action_reason: None,
            index: None,
            depends_on: None,
            before: None,
            after: Some(serde_json::json!({"bucket": "test"})),
            changes: None,
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

    #[test]
    fn test_compute_changes_tag_update() {
        // Test compact diff for tag changes
        let resource_changes = json!([{
            "address": "aws_s3_bucket.example",
            "type": "aws_s3_bucket",
            "name": "example",
            "mode": "managed",
            "change": {
                "actions": ["update"],
                "before": {
                    "bucket": "my-bucket",
                    "tags": {
                        "Environment": "dev",
                        "Owner": "team-a"
                    },
                    "versioning": [{
                        "enabled": false
                    }]
                },
                "after": {
                    "bucket": "my-bucket",
                    "tags": {
                        "Environment": "prod",
                        "Owner": "team-a",
                        "CostCenter": "engineering"
                    },
                    "versioning": [{
                        "enabled": false
                    }]
                },
                "before_sensitive": {},
                "after_sensitive": {}
            }
        }]);

        let sanitized = sanitize_resource_changes(&resource_changes);
        assert_eq!(sanitized.len(), 1);
        assert_eq!(sanitized[0].action, ResourceAction::Update);

        // Check that changes field is populated
        let changes = sanitized[0].changes.as_ref().unwrap();

        // Environment tag changed from "dev" to "prod"
        assert_eq!(
            changes.get("tags.Environment").unwrap(),
            &serde_json::json!({
                "before": "dev",
                "after": "prod",
                "after_unknown": false
            })
        );

        // CostCenter tag was added
        assert_eq!(
            changes.get("tags.CostCenter").unwrap(),
            &serde_json::json!({
                "before": null,
                "after": "engineering",
                "after_unknown": false
            })
        );

        // Should not include unchanged fields
        assert!(changes.get("bucket").is_none());
        assert!(changes.get("versioning").is_none());
        assert!(changes.get("tags.Owner").is_none());
    }

    #[test]
    fn test_compute_changes_no_changes() {
        // Test that no-op actions don't compute changes
        let resource_changes = json!([{
            "address": "aws_s3_bucket.example",
            "type": "aws_s3_bucket",
            "name": "example",
            "mode": "managed",
            "change": {
                "actions": ["no-op"],
                "before": {
                    "bucket": "my-bucket"
                },
                "after": {
                    "bucket": "my-bucket"
                },
                "before_sensitive": {},
                "after_sensitive": {}
            }
        }]);

        let sanitized = sanitize_resource_changes(&resource_changes);
        assert_eq!(sanitized.len(), 1);
        assert_eq!(sanitized[0].action, ResourceAction::NoOp);

        // No changes field for no-op
        assert!(sanitized[0].changes.is_none());
    }

    #[test]
    fn test_compute_changes_create_action() {
        // Test that create actions don't have changes field
        let resource_changes = json!([{
            "address": "aws_s3_bucket.new",
            "type": "aws_s3_bucket",
            "name": "new",
            "mode": "managed",
            "change": {
                "actions": ["create"],
                "after": {
                    "bucket": "new-bucket",
                    "tags": {"env": "prod"}
                },
                "after_sensitive": {}
            }
        }]);

        let sanitized = sanitize_resource_changes(&resource_changes);
        assert_eq!(sanitized.len(), 1);
        assert_eq!(sanitized[0].action, ResourceAction::Create);

        // No changes field for create (no before state)
        assert!(sanitized[0].changes.is_none());
    }

    #[test]
    fn test_compute_changes_nested_object() {
        // Test diff with nested objects
        let resource_changes = json!([{
            "address": "aws_db_instance.example",
            "type": "aws_db_instance",
            "name": "example",
            "mode": "managed",
            "change": {
                "actions": ["update"],
                "before": {
                    "engine": "postgres",
                    "engine_version": "13.7",
                    "backup_retention": {
                        "days": 7,
                        "window": "03:00-04:00"
                    }
                },
                "after": {
                    "engine": "postgres",
                    "engine_version": "14.2",
                    "backup_retention": {
                        "days": 14,
                        "window": "03:00-04:00"
                    }
                },
                "before_sensitive": {},
                "after_sensitive": {}
            }
        }]);

        let sanitized = sanitize_resource_changes(&resource_changes);
        let changes = sanitized[0].changes.as_ref().unwrap();

        // Engine version changed
        assert_eq!(
            changes.get("engine_version").unwrap(),
            &serde_json::json!({
                "before": "13.7",
                "after": "14.2",
                "after_unknown": false
            })
        );

        // Nested field changed
        assert_eq!(
            changes.get("backup_retention.days").unwrap(),
            &serde_json::json!({
                "before": 7,
                "after": 14,
                "after_unknown": false
            })
        );

        // Unchanged fields should not appear
        assert!(changes.get("engine").is_none());
        assert!(changes.get("backup_retention.window").is_none());
    }

    #[test]
    fn test_compute_changes_additions_and_deletions() {
        // Test that additions (null before) and deletions (null after) are captured
        let resource_changes = json!([{
            "address": "aws_s3_bucket.example",
            "type": "aws_s3_bucket",
            "name": "example",
            "mode": "managed",
            "change": {
                "actions": ["update"],
                "before": {
                    "bucket": "my-bucket",
                    "old_tag": "removed",
                    "tags": {
                        "Environment": "dev",
                        "ToBeRemoved": "value"
                    }
                },
                "after": {
                    "bucket": "my-bucket",
                    "new_field": "added",
                    "tags": {
                        "Environment": "dev",
                        "NewTag": "new-value"
                    }
                },
                "before_sensitive": {},
                "after_sensitive": {}
            }
        }]);

        let sanitized = sanitize_resource_changes(&resource_changes);
        assert_eq!(sanitized.len(), 1);

        // Update action should only have changes, not full state
        assert!(sanitized[0].before.is_none());
        assert!(sanitized[0].after.is_none());
        assert!(sanitized[0].changes.is_some());

        let changes = sanitized[0].changes.as_ref().unwrap();

        // Field that was deleted
        assert_eq!(
            changes.get("old_tag").unwrap(),
            &serde_json::json!({
                "before": "removed",
                "after": null,
                "after_unknown": false
            })
        );

        // Field that was added
        assert_eq!(
            changes.get("new_field").unwrap(),
            &serde_json::json!({
                "before": null,
                "after": "added",
                "after_unknown": false
            })
        );

        // Tag that was removed
        assert_eq!(
            changes.get("tags.ToBeRemoved").unwrap(),
            &serde_json::json!({
                "before": "value",
                "after": null,
                "after_unknown": false
            })
        );

        // Tag that was added
        assert_eq!(
            changes.get("tags.NewTag").unwrap(),
            &serde_json::json!({
                "before": null,
                "after": "new-value",
                "after_unknown": false
            })
        );

        // Unchanged fields should not appear
        assert!(changes.get("bucket").is_none());
        assert!(changes.get("tags.Environment").is_none());
    }

    #[test]
    fn test_sensitive_field_additions_and_deletions() {
        // Test that adding/removing sensitive fields shows as [REDACTED]
        let resource_changes = json!([{
            "address": "aws_db_instance.example",
            "type": "aws_db_instance",
            "name": "example",
            "mode": "managed",
            "change": {
                "actions": ["update"],
                "before": {
                    "engine": "postgres",
                    "old_password": "old-secret",
                    "tags": {
                        "public": "value"
                    }
                },
                "after": {
                    "engine": "postgres",
                    "new_password": "new-secret",
                    "tags": {
                        "public": "value",
                        "secret_key": "secret-value"
                    }
                },
                "before_sensitive": {
                    "old_password": true
                },
                "after_sensitive": {
                    "new_password": true,
                    "tags": {
                        "secret_key": true
                    }
                }
            }
        }]);

        let sanitized = sanitize_resource_changes(&resource_changes);
        let changes = sanitized[0].changes.as_ref().unwrap();

        // Sensitive field deletion
        assert_eq!(
            changes.get("old_password").unwrap(),
            &serde_json::json!({
                "before": "[REDACTED]",
                "after": null,
                "after_unknown": false
            })
        );

        // Sensitive field addition
        assert_eq!(
            changes.get("new_password").unwrap(),
            &serde_json::json!({
                "before": null,
                "after": "[REDACTED]",
                "after_unknown": false
            })
        );

        // Sensitive tag addition
        assert_eq!(
            changes.get("tags.secret_key").unwrap(),
            &serde_json::json!({
                "before": null,
                "after": "[REDACTED]",
                "after_unknown": false
            })
        );

        // Unchanged fields shouldn't appear
        assert!(changes.get("engine").is_none());
        assert!(changes.get("tags.public").is_none());
    }

    #[test]
    fn test_action_reason_and_index() {
        // Test that action_reason and index are extracted
        let resource_changes = json!([{
            "address": "aws_instance.web[0]",
            "type": "aws_instance",
            "name": "web",
            "mode": "managed",
            "index": 0,
            "action_reason": "replace_because_cannot_update",
            "change": {
                "actions": ["delete", "create"],
                "before": {
                    "instance_type": "t2.micro",
                    "ami": "ami-old"
                },
                "after": {
                    "instance_type": "t2.micro",
                    "ami": "ami-new"
                },
                "before_sensitive": {},
                "after_sensitive": {}
            }
        }]);

        let sanitized = sanitize_resource_changes(&resource_changes);
        assert_eq!(sanitized.len(), 1);

        // Verify action_reason is captured
        assert_eq!(
            sanitized[0].action_reason.as_ref().unwrap(),
            "replace_because_cannot_update"
        );

        // Verify index is captured
        assert_eq!(sanitized[0].index.as_ref().unwrap(), &serde_json::json!(0));

        // Verify it's a replace action
        assert_eq!(sanitized[0].action, ResourceAction::Replace);
    }

    #[test]
    fn test_for_each_index() {
        // Test string index for for_each
        let resource_changes = json!([{
            "address": "aws_s3_bucket.buckets[\"production\"]",
            "type": "aws_s3_bucket",
            "name": "buckets",
            "mode": "managed",
            "index": "production",
            "change": {
                "actions": ["create"],
                "after": {
                    "bucket": "prod-bucket"
                },
                "after_sensitive": {}
            }
        }]);

        let sanitized = sanitize_resource_changes(&resource_changes);
        assert_eq!(sanitized.len(), 1);

        // Verify string index is captured
        assert_eq!(
            sanitized[0].index.as_ref().unwrap(),
            &serde_json::json!("production")
        );
    }

    #[test]
    fn test_provider_and_depends_on() {
        // Test that provider and depends_on are extracted
        let resource_changes = json!([{
            "address": "aws_s3_bucket.example",
            "type": "aws_s3_bucket",
            "name": "example",
            "mode": "managed",
            "provider_name": "registry.terraform.io/hashicorp/aws",
            "change": {
                "actions": ["create"],
                "after": {
                    "bucket": "my-bucket"
                },
                "after_sensitive": {},
                "after_depends_on": [
                    "aws_iam_role.bucket_role",
                    "aws_kms_key.bucket_key"
                ]
            }
        }]);

        let sanitized = sanitize_resource_changes(&resource_changes);
        assert_eq!(sanitized.len(), 1);

        // Verify provider is captured
        assert_eq!(
            sanitized[0].provider.as_ref().unwrap(),
            "registry.terraform.io/hashicorp/aws"
        );

        // Verify depends_on is captured - for create action, all deps are "added"
        let depends_on = sanitized[0].depends_on.as_ref().unwrap();
        assert_eq!(depends_on.added.len(), 2);
        assert!(depends_on
            .added
            .contains(&"aws_iam_role.bucket_role".to_string()));
        assert!(depends_on
            .added
            .contains(&"aws_kms_key.bucket_key".to_string()));
        assert_eq!(depends_on.removed.len(), 0);
        assert_eq!(depends_on.unchanged.len(), 0);
    }

    #[test]
    fn test_dependency_changes() {
        // Test dependency changes during update
        let resource_changes = json!([{
            "address": "aws_instance.web",
            "type": "aws_instance",
            "name": "web",
            "mode": "managed",
            "change": {
                "actions": ["update"],
                "before": {
                    "instance_type": "t2.micro"
                },
                "after": {
                    "instance_type": "t3.micro"
                },
                "before_sensitive": {},
                "after_sensitive": {},
                "before_depends_on": [
                    "aws_security_group.old",
                    "aws_subnet.main"
                ],
                "after_depends_on": [
                    "aws_security_group.new",
                    "aws_subnet.main"
                ]
            }
        }]);

        let sanitized = sanitize_resource_changes(&resource_changes);
        assert_eq!(sanitized.len(), 1);

        let depends_on = sanitized[0].depends_on.as_ref().unwrap();

        // One dependency added
        assert_eq!(depends_on.added.len(), 1);
        assert_eq!(depends_on.added[0], "aws_security_group.new");

        // One dependency removed
        assert_eq!(depends_on.removed.len(), 1);
        assert_eq!(depends_on.removed[0], "aws_security_group.old");

        // One dependency unchanged
        assert_eq!(depends_on.unchanged.len(), 1);
        assert_eq!(depends_on.unchanged[0], "aws_subnet.main");
    }

    #[test]
    fn test_no_dependencies() {
        // Test resource with no dependencies
        let resource_changes = json!([{
            "address": "aws_s3_bucket.simple",
            "type": "aws_s3_bucket",
            "name": "simple",
            "mode": "managed",
            "change": {
                "actions": ["create"],
                "after": {
                    "bucket": "simple-bucket"
                },
                "after_sensitive": {}
            }
        }]);

        let sanitized = sanitize_resource_changes(&resource_changes);
        assert_eq!(sanitized.len(), 1);

        // No dependencies means None
        assert!(sanitized[0].depends_on.is_none());
    }
}
