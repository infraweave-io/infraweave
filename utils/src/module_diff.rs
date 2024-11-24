use env_defs::{ModuleDiffAddition, ModuleDiffChange, ModuleDiffRemoval};
use hcl::from_str as hcl_from_str;
use hcl::Value as HclValue;
use serde_json::Value as JsonValue;
use std::vec;

// Convert HCL value to serde_json::Value
fn hcl_to_json(hcl_value: &HclValue) -> JsonValue {
    serde_json::to_value(hcl_value).unwrap()
}

fn parse_hcl(input: &str) -> JsonValue {
    let hcl_value: HclValue = hcl_from_str(input).expect("Unable to parse HCL");
    hcl_to_json(&hcl_value)
}

pub fn diff_modules(
    module1: &str,
    module2: &str,
) -> (
    Vec<ModuleDiffAddition>,
    Vec<ModuleDiffChange>,
    Vec<ModuleDiffRemoval>,
) {
    // Parse both HCL strings into JSON-like structures
    let module1: JsonValue = parse_hcl(module1);
    let module2: JsonValue = parse_hcl(module2);

    // Find differences
    diff_values(&module1, &module2, "")
}

// Compare two serde_json::Value objects and collect the differences
fn diff_values(
    value1: &JsonValue,
    value2: &JsonValue,
    path: &str,
) -> (
    Vec<ModuleDiffAddition>,
    Vec<ModuleDiffChange>,
    Vec<ModuleDiffRemoval>,
) {
    let mut additions: Vec<ModuleDiffAddition> = vec![];
    let mut changes: Vec<ModuleDiffChange> = vec![];
    let mut removals: Vec<ModuleDiffRemoval> = vec![];

    match (value1, value2) {
        (JsonValue::Object(map1), JsonValue::Object(map2)) => {
            // Compare existing keys
            for (key, val1) in map1 {
                let new_path = if path.is_empty() {
                    format!("/{}", key)
                } else {
                    format!("{}/{}", path, key)
                };
                if let Some(val2) = map2.get(key) {
                    let (mut add, mut change, mut remove) = diff_values(val1, val2, &new_path);
                    additions.append(&mut add);
                    changes.append(&mut change);
                    removals.append(&mut remove);
                } else {
                    // Key removed
                    if val1.is_object() {
                        for (sub_key, sub_val) in val1.as_object().unwrap() {
                            let sub_path = format!("{}/{}", new_path, sub_key);
                            removals.push(ModuleDiffRemoval {
                                path: sub_path,
                                value: sub_val.clone(),
                            });
                        }
                    } else {
                        removals.push(ModuleDiffRemoval {
                            path: new_path,
                            value: val1.clone(),
                        });
                    }
                }
            }

            // Check for new keys
            for (key, val2) in map2 {
                if !map1.contains_key(key) {
                    let new_path = if path.is_empty() {
                        format!("/{}", key)
                    } else {
                        format!("{}/{}", path, key)
                    };
                    match val2 {
                        JsonValue::Object(_) => {
                            // If it's an object, iterate over its keys and add as individual additions
                            for (sub_key, sub_val) in val2.as_object().unwrap() {
                                let sub_path = format!("{}/{}", new_path, sub_key);
                                additions.push(ModuleDiffAddition {
                                    path: sub_path,
                                    value: sub_val.clone(),
                                });
                            }
                        }
                        _ => {
                            additions.push(ModuleDiffAddition {
                                path: new_path,
                                value: val2.clone(),
                            });
                        }
                    }
                }
            }
        }
        (JsonValue::Array(arr1), JsonValue::Array(arr2)) => {
            if arr1 != arr2 {
                changes.push(ModuleDiffChange {
                    path: path.to_string(),
                    old_value: JsonValue::Array(arr1.clone()),
                    new_value: JsonValue::Array(arr2.clone()),
                });
            }
        }
        _ => {
            if value1 != value2 {
                changes.push(ModuleDiffChange {
                    path: path.to_string(),
                    old_value: value1.clone(),
                    new_value: value2.clone(),
                });
            }
        }
    }

    (additions, changes, removals)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn test_terraform_diff() {
        // Example HCL strings for the test
        let version1 = r#"
        resource "aws_s3_bucket" "my_bucket1" {
          name = var.bucket_name
        }

        variable "bucket_name" {
          default = "hej"
        }
        "#;

        let version2 = r#"
        resource "aws_s3_bucket" "my_bucket1" {
          name = var.bucket_name
        }

        variable "bucket_name" {
          default = "hej2"
        }

        resource "aws_s3_bucket_object" "my_object1" {
          key = "my_object"
        }
        "#;

        let (additions, changes, removals) = diff_modules(version1, version2);

        let expected_additions = vec![ModuleDiffAddition {
            path: "/resource/aws_s3_bucket_object/my_object1".to_string(),
            value: serde_json::json!({"key": "my_object"}),
        }];

        let expected_changes = vec![ModuleDiffChange {
            path: "/variable/bucket_name/default".to_string(),
            old_value: serde_json::json!("hej"),
            new_value: serde_json::json!("hej2"),
        }];

        let expected_removals: Vec<ModuleDiffRemoval> = vec![];

        // Ensure the diffs are as expected
        assert_eq!(additions, expected_additions);
        assert_eq!(changes, expected_changes);
        assert_eq!(removals, expected_removals);
    }

    #[test]
    fn test_terraform_diff_with_multiple_levels() {
        // Example HCL strings with deeper resource structure
        let version1 = r#"
        resource "aws_s3_bucket" "my_bucket1" {
          name = var.bucket_name
        }

        variable "bucket_name" {
          default = "hej"
        }
        "#;

        let version2 = r#"
        resource "aws_s3_bucket" "my_bucket1" {
          name = var.bucket_name
        }

        variable "bucket_name" {
          default = "hej2"
        }

        resource "aws_s3_bucket_object" "my_object1" {
          key = "my_object"
        }
        "#;

        let (additions, changes, removals) = diff_modules(version1, version2);

        let expected_additions = vec![ModuleDiffAddition {
            path: "/resource/aws_s3_bucket_object/my_object1".to_string(),
            value: serde_json::json!({"key": "my_object"}),
        }];

        let expected_changes = vec![ModuleDiffChange {
            path: "/variable/bucket_name/default".to_string(),
            old_value: serde_json::json!("hej"),
            new_value: serde_json::json!("hej2"),
        }];

        let expected_removals: Vec<ModuleDiffRemoval> = vec![];

        // Ensure the diffs are as expected
        assert_eq!(additions, expected_additions);
        assert_eq!(changes, expected_changes);
        assert_eq!(removals, expected_removals);
    }

    #[test]
    fn test_terraform_diff_multiple_additions() {
        // Example HCL strings with multiple additions
        let version1 = r#"
        resource "aws_s3_bucket" "my_bucket1" {
          name = var.bucket_name
        }

        variable "bucket_name" {
          default = "hej"
        }
        "#;

        let version2 = r#"
        resource "aws_s3_bucket" "my_bucket1" {
          name = var.bucket_name
        }

        variable "bucket_name" {
          default = "hej2"
        }

        resource "aws_s3_bucket_object" "my_object1" {
          key = "my_object1"
        }

        resource "aws_s3_bucket_object" "my_object2" {
          key = "my_object2"
        }
        "#;

        let (additions, changes, removals) = diff_modules(version1, version2);

        let expected_additions = vec![
            ModuleDiffAddition {
                path: "/resource/aws_s3_bucket_object/my_object1".to_string(),
                value: serde_json::json!({"key": "my_object1"}),
            },
            ModuleDiffAddition {
                path: "/resource/aws_s3_bucket_object/my_object2".to_string(),
                value: serde_json::json!({"key": "my_object2"}),
            },
        ];

        let expected_changes = vec![ModuleDiffChange {
            path: "/variable/bucket_name/default".to_string(),
            old_value: serde_json::json!("hej"),
            new_value: serde_json::json!("hej2"),
        }];

        let expected_removals: Vec<ModuleDiffRemoval> = vec![];

        // Ensure the diffs are as expected
        assert_eq!(additions, expected_additions);
        assert_eq!(changes, expected_changes);
        assert_eq!(removals, expected_removals);
    }

    #[test]
    fn test_terraform_diff_with_removals() {
        // Example HCL strings with removals
        let version1 = r#"
        resource "aws_s3_bucket" "my_bucket1" {
          name = var.bucket_name
        }
    
        resource "aws_s3_bucket_object" "my_object1" {
          key = "my_object1"
          destination = "my_destination"
        }
    
        variable "bucket_name" {
          default = "hej"
        }
        "#;

        let version2 = r#"
        resource "aws_s3_bucket" "my_bucket1" {
          name = var.bucket_name
        }
    
        variable "bucket_name" {
          default = "hej2"
        }
        "#;

        let (additions, changes, removals) = diff_modules(version1, version2);

        let expected_additions: Vec<ModuleDiffAddition> = vec![];

        let expected_changes = vec![ModuleDiffChange {
            path: "/variable/bucket_name/default".to_string(),
            old_value: serde_json::json!("hej"),
            new_value: serde_json::json!("hej2"),
        }];

        let expected_removals = vec![ModuleDiffRemoval {
            path: "/resource/aws_s3_bucket_object/my_object1".to_string(),
            value: serde_json::json!({
                "key": "my_object1",
                "destination": "my_destination",
            }),
        }];

        // Ensure the diffs are as expected
        assert_eq!(additions, expected_additions);
        assert_eq!(changes, expected_changes);
        assert_eq!(removals, expected_removals);
    }
}
