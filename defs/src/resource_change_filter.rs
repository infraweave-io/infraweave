use crate::resource_change::{ResourceAction, SanitizedResourceChange};
use regex::Regex;
use serde::{Deserialize, Serialize};

/// Rule for filtering resource changes based on path and optional value pattern
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FilterRule {
    /// Path prefix to match (e.g., "tags", "metadata.annotations")
    pub path: String,
    /// Optional regex pattern to match against the last segment of the path
    /// (e.g., "^INFRAWEAVE_" to match tags starting with INFRAWEAVE_)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value_pattern: Option<String>,
    /// Optional regex pattern to match against the resource type
    /// (e.g., "^aws_" to match only AWS resources)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resource_pattern: Option<String>,
}

/// Filter configuration for excluding resource changes based on specific criteria
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ResourceChangeFilter {
    /// Rules for filtering changes. A resource is filtered if ALL changes match at least one rule.
    pub rules: Vec<FilterRule>,
}

impl Default for ResourceChangeFilter {
    fn default() -> Self {
        // Try to read filter from environment variable first
        if let Ok(filter_json) = std::env::var("INFRAWEAVE_RESOURCE_CHANGE_FILTER") {
            if let Ok(filter) = serde_json::from_str::<ResourceChangeFilter>(&filter_json) {
                return filter;
            }
            // If parsing fails, log a warning and fall through to default
            eprintln!(
                "Warning: Failed to parse INFRAWEAVE_RESOURCE_CHANGE_FILTER, using default filter"
            );
        }

        // Default filter: exclude only INFRAWEAVE_* tags from AWS resources
        Self {
            rules: vec![
                FilterRule {
                    path: "tags".to_string(),
                    value_pattern: Some("^INFRAWEAVE_".to_string()),
                    resource_pattern: Some("^aws_".to_string()),
                },
                FilterRule {
                    path: "tags_all".to_string(),
                    value_pattern: Some("^INFRAWEAVE_".to_string()),
                    resource_pattern: Some("^aws_".to_string()),
                },
            ],
        }
    }
}

impl ResourceChangeFilter {
    /// Check if a resource change should be filtered out (returns true if should be excluded)
    pub fn should_filter(&self, change: &SanitizedResourceChange) -> bool {
        // Only filter update/replace actions that have changes
        if !matches!(
            change.action,
            ResourceAction::Update | ResourceAction::Replace
        ) {
            return false;
        }

        let Some(changes) = &change.changes else {
            return false;
        };

        if changes.is_empty() || self.rules.is_empty() {
            return false;
        }

        // Check if all changes match at least one filter rule
        changes.keys().all(|changed_path| {
            self.rules
                .iter()
                .any(|rule| Self::path_matches_rule(changed_path, rule))
        })
    }

    /// Check if a specific change field path should be filtered
    pub fn should_filter_field(&self, field_path: &str) -> bool {
        if self.rules.is_empty() {
            return false;
        }

        self.rules
            .iter()
            .any(|rule| Self::path_matches_rule(field_path, rule))
    }

    /// Check if a specific change field path should be filtered for a given resource type
    pub fn should_filter_field_for_resource(&self, field_path: &str, resource_type: &str) -> bool {
        if self.rules.is_empty() {
            return false;
        }

        self.rules.iter().any(|rule| {
            // Check if resource type matches (if pattern is specified)
            if let Some(ref type_pattern) = rule.resource_pattern {
                let type_matches = Regex::new(type_pattern)
                    .map(|re| re.is_match(resource_type))
                    .unwrap_or_else(|_| resource_type.starts_with(type_pattern));

                if !type_matches {
                    return false;
                }
            }

            // Check if field path matches
            Self::path_matches_rule(field_path, rule)
        })
    }

    /// Filter individual change fields from a resource, returning a modified resource
    /// with filtered fields removed. Returns None if all fields are filtered.
    pub fn filter_change_fields(
        &self,
        change: &SanitizedResourceChange,
    ) -> Option<SanitizedResourceChange> {
        // Only filter update/replace actions that have changes
        if !matches!(
            change.action,
            ResourceAction::Update | ResourceAction::Replace
        ) {
            return Some(change.clone());
        }

        let Some(changes) = &change.changes else {
            return Some(change.clone());
        };

        if changes.is_empty() || self.rules.is_empty() {
            return Some(change.clone());
        }

        // Filter out fields that match filter rules for this resource type
        let filtered_changes: serde_json::Map<String, serde_json::Value> = changes
            .iter()
            .filter(|(path, _)| !self.should_filter_field_for_resource(path, &change.resource_type))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();

        // If all fields were filtered, return None
        if filtered_changes.is_empty() {
            return None;
        }

        // Return modified resource with filtered changes
        let mut modified = change.clone();
        modified.changes = Some(filtered_changes);
        Some(modified)
    }

    /// Check if a changed path matches a filter rule
    fn path_matches_rule(changed_path: &str, rule: &FilterRule) -> bool {
        // Check if path matches the rule's path prefix
        if changed_path == rule.path {
            return rule.value_pattern.is_none();
        }

        if !changed_path.starts_with(&format!("{}.", &rule.path)) {
            return false;
        }

        // If there's a value pattern, check if the value name matches
        match &rule.value_pattern {
            Some(pattern) => {
                let value_name = &changed_path[rule.path.len() + 1..];
                let first_segment = value_name.split('.').next().unwrap_or("");

                // Try to compile and match regex, fall back to prefix match if invalid
                Regex::new(pattern)
                    .map(|re| re.is_match(first_segment))
                    .unwrap_or_else(|_| first_segment.starts_with(pattern))
            }
            None => true, // No pattern means match any value under this path
        }
    }
}

/// Apply a filter to resource changes, removing changes that match the filter criteria
pub fn filter_resource_changes(
    changes: Vec<SanitizedResourceChange>,
    filter: &ResourceChangeFilter,
) -> Vec<SanitizedResourceChange> {
    changes
        .into_iter()
        .filter(|change| !filter.should_filter(change))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resource_change::ResourceMode;
    use serde_json::json;

    #[test]
    fn test_filter_serialization() {
        // Test that filters can be serialized/deserialized to/from JSON
        let filter = ResourceChangeFilter::default();
        let json = serde_json::to_string(&filter).unwrap();
        let deserialized: ResourceChangeFilter = serde_json::from_str(&json).unwrap();
        assert_eq!(filter, deserialized);

        // Test pretty JSON
        let pretty = serde_json::to_string_pretty(&filter).unwrap();
        assert!(pretty.contains("INFRAWEAVE_"));
        assert!(pretty.contains("rules"));
    }

    #[test]
    fn test_filter_from_env_var_format() {
        // Test parsing filter from environment variable format
        let json = r#"{
            "rules": [
                {
                    "path": "tags",
                    "value_pattern": "^INFRAWEAVE_"
                },
                {
                    "path": "tags_all",
                    "value_pattern": "^INFRAWEAVE_"
                }
            ]
        }"#;

        let filter: ResourceChangeFilter = serde_json::from_str(json).unwrap();
        assert_eq!(filter.rules.len(), 2);
        assert_eq!(filter.rules[0].path, "tags");
        assert_eq!(
            filter.rules[0].value_pattern,
            Some("^INFRAWEAVE_".to_string())
        );
    }

    #[test]
    fn test_filter_custom_regex() {
        // Test custom regex pattern
        let json = r#"{
            "rules": [
                {
                    "path": "tags",
                    "value_pattern": "^(INFRAWEAVE_|SYSTEM_)"
                }
            ]
        }"#;

        let filter: ResourceChangeFilter = serde_json::from_str(json).unwrap();

        let change = SanitizedResourceChange {
            address: "aws_s3_bucket.test".to_string(),
            resource_type: "aws_s3_bucket".to_string(),
            name: "test".to_string(),
            mode: ResourceMode::Managed,
            provider: None,
            action: ResourceAction::Update,
            action_reason: None,
            index: None,
            depends_on: None,
            before: None,
            after: None,
            changes: Some({
                let mut map = serde_json::Map::new();
                map.insert(
                    "tags.INFRAWEAVE_MODULE_VERSION".to_string(),
                    json!({"before": "1.0.0", "after": "1.0.1"}),
                );
                map.insert(
                    "tags.SYSTEM_ID".to_string(),
                    json!({"before": "sys-1", "after": "sys-2"}),
                );
                map
            }),
        };

        // Both should be filtered by the custom pattern
        assert!(filter.should_filter(&change));
    }

    #[test]
    fn test_filter_tag_only_changes() {
        // Create two resources: one with only tag changes, one with other changes
        let changes = vec![
            SanitizedResourceChange {
                address: "aws_s3_bucket.tags_only".to_string(),
                resource_type: "aws_s3_bucket".to_string(),
                name: "tags_only".to_string(),
                mode: ResourceMode::Managed,
                provider: None,
                action: ResourceAction::Update,
                action_reason: None,
                index: None,
                depends_on: None,
                before: None,
                after: None,
                changes: Some({
                    let mut map = serde_json::Map::new();
                    map.insert(
                        "tags.Environment".to_string(),
                        json!({"before": "dev", "after": "prod"}),
                    );
                    map.insert(
                        "tags.Owner".to_string(),
                        json!({"before": "team-a", "after": "team-b"}),
                    );
                    map
                }),
            },
            SanitizedResourceChange {
                address: "aws_instance.web".to_string(),
                resource_type: "aws_instance".to_string(),
                name: "web".to_string(),
                mode: ResourceMode::Managed,
                provider: None,
                action: ResourceAction::Update,
                action_reason: None,
                index: None,
                depends_on: None,
                before: None,
                after: None,
                changes: Some({
                    let mut map = serde_json::Map::new();
                    map.insert(
                        "tags.Environment".to_string(),
                        json!({"before": "dev", "after": "prod"}),
                    );
                    map.insert(
                        "instance_type".to_string(),
                        json!({"before": "t2.micro", "after": "t3.micro"}),
                    );
                    map
                }),
            },
        ];

        let filter = ResourceChangeFilter {
            rules: vec![
                FilterRule {
                    path: "tags".to_string(),
                    value_pattern: None,
                    resource_pattern: None,
                },
                FilterRule {
                    path: "tags_all".to_string(),
                    value_pattern: None,
                    resource_pattern: None,
                },
            ],
        };
        let filtered = filter_resource_changes(changes, &filter);

        // Tag-only change should be filtered out
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].address, "aws_instance.web");
    }

    #[test]
    fn test_filter_mixed_changes_not_filtered() {
        // Create a resource with both tag and non-tag changes
        let changes = vec![SanitizedResourceChange {
            address: "aws_s3_bucket.test".to_string(),
            resource_type: "aws_s3_bucket".to_string(),
            name: "test".to_string(),
            mode: ResourceMode::Managed,
            provider: None,
            action: ResourceAction::Update,
            action_reason: None,
            index: None,
            depends_on: None,
            before: None,
            after: None,
            changes: Some({
                let mut map = serde_json::Map::new();
                map.insert(
                    "tags.INFRAWEAVE_MODULE_VERSION".to_string(),
                    json!({"before": "1.0.0", "after": "1.0.1"}),
                );
                map.insert(
                    "tags.INFRAWEAVE_GIT_COMMIT_SHA".to_string(),
                    json!({"before": "abc123", "after": "def456"}),
                );
                map.insert(
                    "versioning.enabled".to_string(),
                    json!({"before": false, "after": true}),
                );
                map
            }),
        }];

        let filter = ResourceChangeFilter {
            rules: vec![
                FilterRule {
                    path: "tags".to_string(),
                    value_pattern: None,
                    resource_pattern: None,
                },
                FilterRule {
                    path: "tags_all".to_string(),
                    value_pattern: None,
                    resource_pattern: None,
                },
            ],
        };
        let filtered = filter_resource_changes(changes, &filter);

        // Mixed changes should NOT be filtered out
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].address, "aws_s3_bucket.test");
    }

    #[test]
    fn test_filter_create_action_not_filtered() {
        // Create actions should never be filtered
        let changes = vec![SanitizedResourceChange {
            address: "aws_s3_bucket.new".to_string(),
            resource_type: "aws_s3_bucket".to_string(),
            name: "new".to_string(),
            mode: ResourceMode::Managed,
            provider: None,
            action: ResourceAction::Create,
            action_reason: None,
            index: None,
            depends_on: None,
            before: None,
            after: Some(json!({
                "bucket": "new-bucket",
                "tags": {
                    "INFRAWEAVE_DEPLOYMENT_ID": "dep-123",
                    "INFRAWEAVE_MODULE_VERSION": "1.0.0",
                    "INFRAWEAVE_GIT_COMMIT_SHA": "abc123"
                }
            })),
            changes: None,
        }];

        let filter = ResourceChangeFilter {
            rules: vec![
                FilterRule {
                    path: "tags".to_string(),
                    value_pattern: None,
                    resource_pattern: None,
                },
                FilterRule {
                    path: "tags_all".to_string(),
                    value_pattern: None,
                    resource_pattern: None,
                },
            ],
        };
        let filtered = filter_resource_changes(changes, &filter);

        // Create action should not be filtered
        assert_eq!(filtered.len(), 1);
    }

    #[test]
    fn test_filter_delete_action_not_filtered() {
        // Delete actions should never be filtered
        let changes = vec![SanitizedResourceChange {
            address: "aws_s3_bucket.old".to_string(),
            resource_type: "aws_s3_bucket".to_string(),
            name: "old".to_string(),
            mode: ResourceMode::Managed,
            provider: None,
            action: ResourceAction::Delete,
            action_reason: None,
            index: None,
            depends_on: None,
            before: Some(json!({
                "bucket": "old-bucket",
                "tags": {
                    "INFRAWEAVE_DEPLOYMENT_ID": "dep-456",
                    "INFRAWEAVE_MODULE_VERSION": "0.9.0"
                }
            })),
            after: None,
            changes: None,
        }];

        let filter = ResourceChangeFilter {
            rules: vec![
                FilterRule {
                    path: "tags".to_string(),
                    value_pattern: None,
                    resource_pattern: None,
                },
                FilterRule {
                    path: "tags_all".to_string(),
                    value_pattern: None,
                    resource_pattern: None,
                },
            ],
        };
        let filtered = filter_resource_changes(changes, &filter);

        // Delete action should not be filtered
        assert_eq!(filtered.len(), 1);
    }

    #[test]
    fn test_filter_no_op_not_filtered() {
        // No-op actions should not be filtered
        let changes = vec![SanitizedResourceChange {
            address: "aws_s3_bucket.unchanged".to_string(),
            resource_type: "aws_s3_bucket".to_string(),
            name: "unchanged".to_string(),
            mode: ResourceMode::Managed,
            provider: None,
            action: ResourceAction::NoOp,
            action_reason: None,
            index: None,
            depends_on: None,
            before: None,
            after: None,
            changes: None,
        }];

        let filter = ResourceChangeFilter {
            rules: vec![
                FilterRule {
                    path: "tags".to_string(),
                    value_pattern: None,
                    resource_pattern: None,
                },
                FilterRule {
                    path: "tags_all".to_string(),
                    value_pattern: None,
                    resource_pattern: None,
                },
            ],
        };
        let filtered = filter_resource_changes(changes, &filter);

        // No-op should not be filtered
        assert_eq!(filtered.len(), 1);
    }

    #[test]
    fn test_filter_custom_paths() {
        // Test custom ignore paths
        let changes = vec![
            SanitizedResourceChange {
                address: "resource.metadata_only".to_string(),
                resource_type: "kubernetes_deployment".to_string(),
                name: "metadata_only".to_string(),
                mode: ResourceMode::Managed,
                provider: None,
                action: ResourceAction::Update,
                action_reason: None,
                index: None,
                depends_on: None,
                before: None,
                after: None,
                changes: Some({
                    let mut map = serde_json::Map::new();
                    map.insert(
                        "metadata.labels.version".to_string(),
                        json!({"before": "1.0", "after": "1.1"}),
                    );
                    map.insert(
                        "metadata.annotations.updated".to_string(),
                        json!({"before": "2024-01-01", "after": "2024-01-02"}),
                    );
                    map
                }),
            },
            SanitizedResourceChange {
                address: "resource.spec_changed".to_string(),
                resource_type: "kubernetes_deployment".to_string(),
                name: "spec_changed".to_string(),
                mode: ResourceMode::Managed,
                provider: None,
                action: ResourceAction::Update,
                action_reason: None,
                index: None,
                depends_on: None,
                before: None,
                after: None,
                changes: Some({
                    let mut map = serde_json::Map::new();
                    map.insert(
                        "metadata.labels.version".to_string(),
                        json!({"before": "1.0", "after": "1.1"}),
                    );
                    map.insert(
                        "spec.replicas".to_string(),
                        json!({"before": 1, "after": 3}),
                    );
                    map
                }),
            },
        ];

        let filter = ResourceChangeFilter {
            rules: vec![
                FilterRule {
                    path: "metadata.labels".to_string(),
                    value_pattern: None,
                    resource_pattern: None,
                },
                FilterRule {
                    path: "metadata.annotations".to_string(),
                    value_pattern: None,
                    resource_pattern: None,
                },
            ],
        };
        let filtered = filter_resource_changes(changes, &filter);

        // Metadata-only change should be filtered out
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].address, "resource.spec_changed");
    }

    #[test]
    fn test_filter_multiple_ignore_paths() {
        // Test with multiple ignore paths - AWS tags and tags_all
        let changes = vec![
            SanitizedResourceChange {
                address: "aws_s3_bucket.tags_only".to_string(),
                resource_type: "aws_s3_bucket".to_string(),
                name: "tags_only".to_string(),
                mode: ResourceMode::Managed,
                provider: None,
                action: ResourceAction::Update,
                action_reason: None,
                index: None,
                depends_on: None,
                before: None,
                after: None,
                changes: Some({
                    let mut map = serde_json::Map::new();
                    map.insert(
                        "tags.Environment".to_string(),
                        json!({"before": "dev", "after": "prod"}),
                    );
                    map
                }),
            },
            SanitizedResourceChange {
                address: "aws_s3_bucket.tags_all_only".to_string(),
                resource_type: "aws_s3_bucket".to_string(),
                name: "tags_all_only".to_string(),
                mode: ResourceMode::Managed,
                provider: None,
                action: ResourceAction::Update,
                action_reason: None,
                index: None,
                depends_on: None,
                before: None,
                after: None,
                changes: Some({
                    let mut map = serde_json::Map::new();
                    map.insert(
                        "tags_all.Owner".to_string(),
                        json!({"before": "alice", "after": "bob"}),
                    );
                    map
                }),
            },
            SanitizedResourceChange {
                address: "aws_s3_bucket.real_change".to_string(),
                resource_type: "aws_s3_bucket".to_string(),
                name: "real_change".to_string(),
                mode: ResourceMode::Managed,
                provider: None,
                action: ResourceAction::Update,
                action_reason: None,
                index: None,
                depends_on: None,
                before: None,
                after: None,
                changes: Some({
                    let mut map = serde_json::Map::new();
                    map.insert(
                        "tags.Environment".to_string(),
                        json!({"before": "dev", "after": "prod"}),
                    );
                    map.insert(
                        "bucket".to_string(),
                        json!({"before": "old-name", "after": "new-name"}),
                    );
                    map
                }),
            },
        ];

        let filter = ResourceChangeFilter {
            rules: vec![
                FilterRule {
                    path: "tags".to_string(),
                    value_pattern: None,
                    resource_pattern: None,
                },
                FilterRule {
                    path: "tags_all".to_string(),
                    value_pattern: None,
                    resource_pattern: None,
                },
            ],
        };
        let filtered = filter_resource_changes(changes, &filter);

        // Both tags and tags_all should be filtered
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].address, "aws_s3_bucket.real_change");
    }

    #[test]
    fn test_filter_empty_changes() {
        // Resources with no changes should not be filtered (shouldn't happen, but be safe)
        let changes = vec![SanitizedResourceChange {
            address: "aws_s3_bucket.empty".to_string(),
            resource_type: "aws_s3_bucket".to_string(),
            name: "empty".to_string(),
            mode: ResourceMode::Managed,
            provider: None,
            action: ResourceAction::Update,
            action_reason: None,
            index: None,
            depends_on: None,
            before: None,
            after: None,
            changes: Some(serde_json::Map::new()),
        }];

        let filter = ResourceChangeFilter {
            rules: vec![
                FilterRule {
                    path: "tags".to_string(),
                    value_pattern: None,
                    resource_pattern: None,
                },
                FilterRule {
                    path: "tags_all".to_string(),
                    value_pattern: None,
                    resource_pattern: None,
                },
            ],
        };
        let filtered = filter_resource_changes(changes, &filter);

        // Empty changes should not be filtered
        assert_eq!(filtered.len(), 1);
    }

    #[test]
    fn test_filter_none_changes() {
        // Resources with None changes should not be filtered
        let changes = vec![SanitizedResourceChange {
            address: "aws_s3_bucket.none".to_string(),
            resource_type: "aws_s3_bucket".to_string(),
            name: "none".to_string(),
            mode: ResourceMode::Managed,
            provider: None,
            action: ResourceAction::Update,
            action_reason: None,
            index: None,
            depends_on: None,
            before: None,
            after: None,
            changes: None,
        }];

        let filter = ResourceChangeFilter {
            rules: vec![
                FilterRule {
                    path: "tags".to_string(),
                    value_pattern: None,
                    resource_pattern: None,
                },
                FilterRule {
                    path: "tags_all".to_string(),
                    value_pattern: None,
                    resource_pattern: None,
                },
            ],
        };
        let filtered = filter_resource_changes(changes, &filter);

        // None changes should not be filtered
        assert_eq!(filtered.len(), 1);
    }

    #[test]
    fn test_filter_default_filters_infraweave_tags() {
        // Default filter should filter only INFRAWEAVE_ prefixed tags
        let changes = vec![SanitizedResourceChange {
            address: "aws_s3_bucket.test".to_string(),
            resource_type: "aws_s3_bucket".to_string(),
            name: "test".to_string(),
            mode: ResourceMode::Managed,
            provider: None,
            action: ResourceAction::Update,
            action_reason: None,
            index: None,
            depends_on: None,
            before: None,
            after: None,
            changes: Some({
                let mut map = serde_json::Map::new();
                map.insert(
                    "tags.INFRAWEAVE_DEPLOYMENT_ID".to_string(),
                    json!({"before": "dep-123", "after": "dep-456"}),
                );
                map.insert(
                    "tags.INFRAWEAVE_MODULE_VERSION".to_string(),
                    json!({"before": "1.0.0", "after": "1.0.1"}),
                );
                map.insert(
                    "tags.INFRAWEAVE_GIT_COMMIT_SHA".to_string(),
                    json!({"before": "abc123", "after": "def456"}),
                );
                map
            }),
        }];

        let filter = ResourceChangeFilter::default();
        let filtered = filter_resource_changes(changes, &filter);

        // Only INFRAWEAVE_ tag changes should be filtered out
        assert_eq!(filtered.len(), 0);
    }

    #[test]
    fn test_filter_no_filter() {
        // Empty filter should not filter anything
        let changes = vec![SanitizedResourceChange {
            address: "aws_s3_bucket.test".to_string(),
            resource_type: "aws_s3_bucket".to_string(),
            name: "test".to_string(),
            mode: ResourceMode::Managed,
            provider: None,
            action: ResourceAction::Update,
            action_reason: None,
            index: None,
            depends_on: None,
            before: None,
            after: None,
            changes: Some({
                let mut map = serde_json::Map::new();
                map.insert(
                    "tags.INFRAWEAVE_DEPLOYMENT_ID".to_string(),
                    json!({"before": "dep-123", "after": "dep-456"}),
                );
                map.insert(
                    "tags.INFRAWEAVE_GIT_COMMIT_SHA".to_string(),
                    json!({"before": "abc123", "after": "def456"}),
                );
                map
            }),
        }];

        let filter = ResourceChangeFilter { rules: vec![] };
        let filtered = filter_resource_changes(changes, &filter);

        // Empty filter should not filter anything
        assert_eq!(filtered.len(), 1);
    }

    #[test]
    fn test_filter_infraweave_tags_only() {
        // Create resources with only INFRAWEAVE_ tag changes
        let changes = vec![
            SanitizedResourceChange {
                address: "aws_s3_bucket.infraweave_only".to_string(),
                resource_type: "aws_s3_bucket".to_string(),
                name: "infraweave_only".to_string(),
                mode: ResourceMode::Managed,
                provider: None,
                action: ResourceAction::Update,
                action_reason: None,
                index: None,
                depends_on: None,
                before: None,
                after: None,
                changes: Some({
                    let mut map = serde_json::Map::new();
                    map.insert(
                        "tags.INFRAWEAVE_MODULE_VERSION".to_string(),
                        json!({"before": "1.0.0", "after": "1.0.1"}),
                    );
                    map.insert(
                        "tags.INFRAWEAVE_GIT_COMMIT_SHA".to_string(),
                        json!({"before": "abc123", "after": "def456"}),
                    );
                    map
                }),
            },
            SanitizedResourceChange {
                address: "aws_instance.web".to_string(),
                resource_type: "aws_instance".to_string(),
                name: "web".to_string(),
                mode: ResourceMode::Managed,
                provider: None,
                action: ResourceAction::Update,
                action_reason: None,
                index: None,
                depends_on: None,
                before: None,
                after: None,
                changes: Some({
                    let mut map = serde_json::Map::new();
                    map.insert(
                        "tags.INFRAWEAVE_GIT_COMMITTER_EMAIL".to_string(),
                        json!({"before": "old@example.com", "after": "new@example.com"}),
                    );
                    map.insert(
                        "instance_type".to_string(),
                        json!({"before": "t2.micro", "after": "t3.micro"}),
                    );
                    map
                }),
            },
        ];

        let filter = ResourceChangeFilter::default();
        let filtered = filter_resource_changes(changes, &filter);

        // Only INFRAWEAVE_ tag changes should be filtered out
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].address, "aws_instance.web");
    }

    #[test]
    fn test_filter_infraweave_mixed_with_user_tags() {
        // Test resource with both INFRAWEAVE_ and user tags changing
        let changes = vec![SanitizedResourceChange {
            address: "aws_s3_bucket.mixed".to_string(),
            resource_type: "aws_s3_bucket".to_string(),
            name: "mixed".to_string(),
            mode: ResourceMode::Managed,
            provider: None,
            action: ResourceAction::Update,
            action_reason: None,
            index: None,
            depends_on: None,
            before: None,
            after: None,
            changes: Some({
                let mut map = serde_json::Map::new();
                map.insert(
                    "tags.INFRAWEAVE_MODULE_VERSION".to_string(),
                    json!({"before": "1.0.0", "after": "1.0.1"}),
                );
                map.insert(
                    "tags.Environment".to_string(),
                    json!({"before": "dev", "after": "prod"}),
                );
                map
            }),
        }];

        let filter = ResourceChangeFilter::default();
        let filtered = filter_resource_changes(changes, &filter);

        // Should NOT be filtered because there's a non-INFRAWEAVE_ tag change
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].address, "aws_s3_bucket.mixed");
    }

    #[test]
    fn test_filter_infraweave_mixed_with_attribute_changes() {
        // Test resource with INFRAWEAVE_ tags and other attribute changes
        let changes = vec![SanitizedResourceChange {
            address: "aws_s3_bucket.mixed".to_string(),
            resource_type: "aws_s3_bucket".to_string(),
            name: "mixed".to_string(),
            mode: ResourceMode::Managed,
            provider: None,
            action: ResourceAction::Update,
            action_reason: None,
            index: None,
            depends_on: None,
            before: None,
            after: None,
            changes: Some({
                let mut map = serde_json::Map::new();
                map.insert(
                    "tags.INFRAWEAVE_GIT_COMMIT_SHA".to_string(),
                    json!({"before": "abc123", "after": "def456"}),
                );
                map.insert(
                    "versioning.enabled".to_string(),
                    json!({"before": false, "after": true}),
                );
                map
            }),
        }];

        let filter = ResourceChangeFilter::default();
        let filtered = filter_resource_changes(changes, &filter);

        // Should NOT be filtered because there's a non-tag change
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].address, "aws_s3_bucket.mixed");
    }

    #[test]
    fn test_filter_user_tags_not_filtered_with_infraweave_filter() {
        // Test that user tags are NOT filtered by the INFRAWEAVE_ filter
        let changes = vec![SanitizedResourceChange {
            address: "aws_s3_bucket.user_tags".to_string(),
            resource_type: "aws_s3_bucket".to_string(),
            name: "user_tags".to_string(),
            mode: ResourceMode::Managed,
            provider: None,
            action: ResourceAction::Update,
            action_reason: None,
            index: None,
            depends_on: None,
            before: None,
            after: None,
            changes: Some({
                let mut map = serde_json::Map::new();
                map.insert(
                    "tags.Environment".to_string(),
                    json!({"before": "dev", "after": "prod"}),
                );
                map.insert(
                    "tags.Owner".to_string(),
                    json!({"before": "team-a", "after": "team-b"}),
                );
                map
            }),
        }];

        let filter = ResourceChangeFilter::default();
        let filtered = filter_resource_changes(changes, &filter);

        // Should NOT be filtered - user tags are important
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].address, "aws_s3_bucket.user_tags");
    }

    #[test]
    fn test_filter_from_env_var() {
        // Test loading filter from environment variable
        let custom_filter = ResourceChangeFilter {
            rules: vec![FilterRule {
                path: "metadata.annotations".to_string(),
                value_pattern: Some("^kubectl.kubernetes.io/".to_string()),
                resource_pattern: None,
            }],
        };

        let filter_json = serde_json::to_string(&custom_filter).unwrap();
        std::env::set_var("INFRAWEAVE_RESOURCE_CHANGE_FILTER", &filter_json);

        let loaded_filter = ResourceChangeFilter::default();
        assert_eq!(loaded_filter.rules.len(), 1);
        assert_eq!(loaded_filter.rules[0].path, "metadata.annotations");
        assert_eq!(
            loaded_filter.rules[0].value_pattern,
            Some("^kubectl.kubernetes.io/".to_string())
        );

        std::env::remove_var("INFRAWEAVE_RESOURCE_CHANGE_FILTER");
    }

    #[test]
    fn test_filter_from_env_var_invalid_json() {
        // Test that invalid JSON falls back to default
        std::env::set_var("INFRAWEAVE_RESOURCE_CHANGE_FILTER", "invalid json");

        let filter = ResourceChangeFilter::default();
        // Should fall back to default rules
        assert_eq!(filter.rules.len(), 2);
        assert_eq!(filter.rules[0].path, "tags");
        assert_eq!(filter.rules[1].path, "tags_all");

        std::env::remove_var("INFRAWEAVE_RESOURCE_CHANGE_FILTER");
    }

    #[test]
    fn test_filter_default_when_no_env_var() {
        // Ensure env var is not set
        std::env::remove_var("INFRAWEAVE_RESOURCE_CHANGE_FILTER");

        let filter = ResourceChangeFilter::default();
        // Should use default rules
        assert_eq!(filter.rules.len(), 2);
        assert_eq!(filter.rules[0].path, "tags");
        assert_eq!(
            filter.rules[0].value_pattern,
            Some("^INFRAWEAVE_".to_string())
        );
        assert_eq!(filter.rules[1].path, "tags_all");
        assert_eq!(
            filter.rules[1].value_pattern,
            Some("^INFRAWEAVE_".to_string())
        );
    }

    #[test]
    fn test_filter_change_fields() {
        // Test that individual fields are filtered while keeping the resource
        let change = SanitizedResourceChange {
            address: "aws_s3_bucket.test".to_string(),
            resource_type: "aws_s3_bucket".to_string(),
            name: "test".to_string(),
            mode: ResourceMode::Managed,
            provider: None,
            action: ResourceAction::Replace,
            action_reason: Some("replace_because_cannot_update".to_string()),
            index: None,
            depends_on: None,
            before: None,
            after: None,
            changes: Some({
                let mut map = serde_json::Map::new();
                map.insert(
                    "tags_all.INFRAWEAVE_MANAGED".to_string(),
                    json!({"before": null, "after": "true"}),
                );
                map.insert(
                    "cors_rule".to_string(),
                    json!({"before": [], "after": null}),
                );
                map.insert(
                    "versioning".to_string(),
                    json!({"before": [{"enabled": true}], "after": null}),
                );
                map
            }),
        };

        let filter = ResourceChangeFilter::default();
        let filtered = filter.filter_change_fields(&change);

        assert!(filtered.is_some());
        let filtered_change = filtered.unwrap();

        // Should still have the resource
        assert_eq!(filtered_change.address, "aws_s3_bucket.test");

        // Should have filtered out INFRAWEAVE_MANAGED but kept others
        let changes = filtered_change.changes.unwrap();
        assert_eq!(changes.len(), 2);
        assert!(!changes.contains_key("tags_all.INFRAWEAVE_MANAGED"));
        assert!(changes.contains_key("cors_rule"));
        assert!(changes.contains_key("versioning"));
    }

    #[test]
    fn test_filter_change_fields_all_filtered() {
        // Test that resource is removed if all fields are filtered
        let change = SanitizedResourceChange {
            address: "aws_s3_bucket.test".to_string(),
            resource_type: "aws_s3_bucket".to_string(),
            name: "test".to_string(),
            mode: ResourceMode::Managed,
            provider: None,
            action: ResourceAction::Update,
            action_reason: None,
            index: None,
            depends_on: None,
            before: None,
            after: None,
            changes: Some({
                let mut map = serde_json::Map::new();
                map.insert(
                    "tags.INFRAWEAVE_MODULE_VERSION".to_string(),
                    json!({"before": "1.0.0", "after": "1.0.1"}),
                );
                map.insert(
                    "tags_all.INFRAWEAVE_DEPLOYMENT_ID".to_string(),
                    json!({"before": "dep-1", "after": "dep-2"}),
                );
                map
            }),
        };

        let filter = ResourceChangeFilter::default();
        let filtered = filter.filter_change_fields(&change);

        // Should be None since all fields were filtered
        assert!(filtered.is_none());
    }

    #[test]
    fn test_field_filtering_scenario_all_resources_filtered() {
        // Scenario 1: 10 resources with only INFRAWEAVE_* tag changes
        // Expected: "All 10 resource changes were filtered out."
        let mut changes = Vec::new();
        for i in 0..10 {
            changes.push(SanitizedResourceChange {
                address: format!("aws_s3_bucket.bucket_{}", i),
                resource_type: "aws_s3_bucket".to_string(),
                name: format!("bucket_{}", i),
                mode: ResourceMode::Managed,
                provider: None,
                action: ResourceAction::Update,
                action_reason: None,
                index: None,
                depends_on: None,
                before: None,
                after: None,
                changes: Some({
                    let mut map = serde_json::Map::new();
                    map.insert(
                        "tags.INFRAWEAVE_MODULE_VERSION".to_string(),
                        json!({"before": "1.0.0", "after": "1.0.1"}),
                    );
                    map.insert(
                        "tags_all.INFRAWEAVE_DEPLOYMENT_ID".to_string(),
                        json!({"before": "dep-1", "after": "dep-2"}),
                    );
                    map
                }),
            });
        }

        let filter = ResourceChangeFilter::default();
        let filtered: Vec<_> = changes
            .iter()
            .filter_map(|c| filter.filter_change_fields(c))
            .collect();

        // All 10 should be filtered out
        assert_eq!(filtered.len(), 0);
        assert_eq!(changes.len(), 10);
    }

    #[test]
    fn test_field_filtering_scenario_partial_filtering() {
        // Scenario 2: 10 resources total
        // - 8 resources with only INFRAWEAVE_* tag changes
        // - 2 resources with INFRAWEAVE_* tags AND name changes
        // Expected: Show 2 resources with name changes (without INFRAWEAVE fields)
        //           Mention 8 resources filtered
        let mut changes = Vec::new();

        // 8 resources with only INFRAWEAVE_* tags
        for i in 0..8 {
            changes.push(SanitizedResourceChange {
                address: format!("aws_s3_bucket.bucket_{}", i),
                resource_type: "aws_s3_bucket".to_string(),
                name: format!("bucket_{}", i),
                mode: ResourceMode::Managed,
                provider: None,
                action: ResourceAction::Update,
                action_reason: None,
                index: None,
                depends_on: None,
                before: None,
                after: None,
                changes: Some({
                    let mut map = serde_json::Map::new();
                    map.insert(
                        "tags.INFRAWEAVE_MODULE_VERSION".to_string(),
                        json!({"before": "1.0.0", "after": "1.0.1"}),
                    );
                    map
                }),
            });
        }

        // 2 resources with both INFRAWEAVE_* tags and name changes
        for i in 8..10 {
            changes.push(SanitizedResourceChange {
                address: format!("aws_s3_bucket.bucket_{}", i),
                resource_type: "aws_s3_bucket".to_string(),
                name: format!("bucket_{}", i),
                mode: ResourceMode::Managed,
                provider: None,
                action: ResourceAction::Update,
                action_reason: None,
                index: None,
                depends_on: None,
                before: None,
                after: None,
                changes: Some({
                    let mut map = serde_json::Map::new();
                    map.insert(
                        "tags.INFRAWEAVE_MODULE_VERSION".to_string(),
                        json!({"before": "1.0.0", "after": "1.0.1"}),
                    );
                    map.insert(
                        "bucket".to_string(),
                        json!({"before": format!("old-name-{}", i), "after": format!("new-name-{}", i)}),
                    );
                    map
                }),
            });
        }

        let filter = ResourceChangeFilter::default();
        let filtered: Vec<_> = changes
            .iter()
            .filter_map(|c| filter.filter_change_fields(c))
            .collect();

        // Should have 2 resources remaining (the ones with name changes)
        assert_eq!(filtered.len(), 2);
        assert_eq!(changes.len(), 10);

        // Verify the INFRAWEAVE fields were filtered out from the remaining resources
        for change in &filtered {
            let changes_map = change.changes.as_ref().unwrap();
            // Should have the bucket change but not INFRAWEAVE tags
            assert!(changes_map.contains_key("bucket"));
            assert!(!changes_map.contains_key("tags.INFRAWEAVE_MODULE_VERSION"));
            assert_eq!(changes_map.len(), 1); // Only bucket field
        }
    }
}
