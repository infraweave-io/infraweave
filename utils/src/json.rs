use heck::ToSnakeCase;
use serde_json::{Map, Value};

// Convert first-level keys to snake_case and leave nested levels unchanged
pub fn convert_first_level_keys_to_snake_case(value: &Value) -> Value {
    match value {
        Value::Object(map) => {
            let mut new_map = Map::new();
            for (key, val) in map {
                let snake_case_key = key.to_snake_case();
                // Only convert first-level keys to snake_case, leave nested structures as-is
                new_map.insert(snake_case_key, val.clone());
            }
            Value::Object(new_map)
        }
        // For arrays, apply the conversion recursively to each element
        Value::Array(arr) => Value::Array(
            arr.iter()
                .map(convert_first_level_keys_to_snake_case)
                .collect(),
        ),
        // For other types (String, Number, etc.), return the value as-is
        _ => value.clone(),
    }
}

pub fn flatten_and_convert_first_level_keys_to_snake_case(value: &Value, prefix: &str) -> Value {
    let mut flat_map = Map::new();

    if let Value::Object(map) = value {
        for (key, val) in map {
            // Convert the first-level key to snake_case
            let snake_case_key = key.to_snake_case();

            if let Value::Object(child_map) = val {
                // For nested objects, flatten their first-level keys
                for (child_key, child_val) in child_map {
                    let snake_case_child_key = child_key.to_snake_case();
                    let new_key = if prefix.is_empty() {
                        format!("{}__{}", snake_case_key, snake_case_child_key)
                    } else {
                        format!("{}__{}__{}", prefix, snake_case_key, snake_case_child_key)
                    };
                    flat_map.insert(new_key, child_val.clone());
                }
            } else {
                // For non-object values, create the new key by concatenating with the prefix if present
                let new_key = if prefix.is_empty() {
                    snake_case_key.clone()
                } else {
                    format!("{}__{}", prefix, snake_case_key)
                };
                flat_map.insert(new_key, val.clone());
            }
        }
    }

    Value::Object(flat_map)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use serde_json::json;
    use serde_json::Value;

    // Helper function to sort the JSON keys for consistent comparison
    fn sorted_json(val: &Value) -> Value {
        match val {
            Value::Object(map) => {
                let mut sorted_map: Vec<_> = map.iter().collect();
                sorted_map.sort_by(|a, b| a.0.cmp(b.0)); // Sort by key
                Value::Object(
                    sorted_map
                        .into_iter()
                        .map(|(k, v)| (k.clone(), sorted_json(v)))
                        .collect(),
                )
            }
            Value::Array(arr) => Value::Array(arr.iter().map(sorted_json).collect()),
            _ => val.clone(),
        }
    }

    #[test]
    fn test_to_snake_case_common_services() {
        assert_eq!("EC2Id".to_snake_case(), "ec2_id");
        assert_eq!("SQLDatabase".to_snake_case(), "sql_database");
        assert_eq!("HTTPServer".to_snake_case(), "http_server");
        assert_eq!("IPAddress".to_snake_case(), "ip_address");
        assert_eq!("MyAPIServer".to_snake_case(), "my_api_server");
        assert_eq!("userID".to_snake_case(), "user_id");
        assert_eq!("UUIDGenerator".to_snake_case(), "uuid_generator");
        assert_eq!("APIGateway".to_snake_case(), "api_gateway");
    }

    #[test]
    fn test_convert_first_level_keys_to_snake_case() {
        let generated_variable_collection = convert_first_level_keys_to_snake_case(&json!({
            "bucketName": "some-bucket",
            "longerNameHere": "long-value",
            "nestedKeyHere": {
                "nestedKey": "nestedValue"
            },
            "withNumbers123": "value",
            "listWithEntries": [1,2],
            "lowercaseonly": "value",
            "UPPERCASEONLY": "value",
            "UPPERCASEAndlowercase": "value",
            "EC2Id": "i-1234567890abcdef0",
        }));

        let expected_variable_collection = json!({
            "bucket_name": "some-bucket",
            "longer_name_here": "long-value",
            "nested_key_here": {
                "nestedKey": "nestedValue"
            },
            "with_numbers123": "value",
            "list_with_entries": [1,2],
            "lowercaseonly": "value",
            "uppercaseonly": "value",
            "uppercase_andlowercase": "value",
            "ec2_id": "i-1234567890abcdef0",
        });

        assert_eq!(
            serde_json::to_string_pretty(&sorted_json(&generated_variable_collection)).unwrap(),
            serde_json::to_string_pretty(&sorted_json(&expected_variable_collection)).unwrap()
        );
    }

    #[test]
    fn test_convert_first_level_keys_to_snake_case_preserves_null() {
        // Test that null values are preserved when converting keys to snake_case
        let generated_variable_collection = convert_first_level_keys_to_snake_case(&json!({
            "myVar": null,
            "anotherVar": "some-value",
            "thirdVar": null,
        }));

        let expected_variable_collection = json!({
            "my_var": null,
            "another_var": "some-value",
            "third_var": null,
        });

        assert_eq!(
            serde_json::to_string_pretty(&sorted_json(&generated_variable_collection)).unwrap(),
            serde_json::to_string_pretty(&sorted_json(&expected_variable_collection)).unwrap()
        );

        // Explicitly verify null values are present and actually null
        let result_obj = generated_variable_collection.as_object().unwrap();
        assert!(result_obj.get("my_var").is_some(), "my_var should exist");
        assert_eq!(
            result_obj.get("my_var").unwrap(),
            &Value::Null,
            "my_var should be null"
        );
        assert!(
            result_obj.get("third_var").is_some(),
            "third_var should exist"
        );
        assert_eq!(
            result_obj.get("third_var").unwrap(),
            &Value::Null,
            "third_var should be null"
        );
    }

    #[test]
    fn test_flatten_and_convert_first_level_keys_to_snake_case() {
        let generated_variable_collection = flatten_and_convert_first_level_keys_to_snake_case(
            &json!({
                "bucket": {
                    "bucketName": "some-bucket",
                    "longerNameHere": "long-value",
                    "nestedKeyHere": {
                        "nestedKey": "nestedValue",
                        "anotherNestedKey": {
                            "anotherNestedKey": "anotherNestedValue"
                        }
                    },
                    "withNumbers123": "value",
                    "listWithEntries": [1,2],
                    "lowercaseonly": "value",
                    "UPPERCASEONLY": "value",
                    "UPPERCASEAndlowercase": "value",
                },
                "eksCluster": {
                    "clusterName": "my-cluster",
                    "nodeGroup": {
                        "nodeGroupName": "my-node-group",
                        "instanceType": "t3.medium"
                    }
                }
            }),
            "",
        );

        // Only first-level keys are converted
        let expected_variable_collection = json!({
            "bucket__bucket_name": "some-bucket",
            "bucket__longer_name_here": "long-value",
            "bucket__nested_key_here": {
                "nestedKey": "nestedValue",
                "anotherNestedKey": {
                    "anotherNestedKey": "anotherNestedValue"
                }
            },
            "bucket__with_numbers123": "value",
            "bucket__list_with_entries": [1, 2],
            "bucket__lowercaseonly": "value",
            "bucket__uppercaseonly": "value",
            "bucket__uppercase_andlowercase": "value",
            "eks_cluster__cluster_name": "my-cluster",
            "eks_cluster__node_group": {
                "nodeGroupName": "my-node-group",
                "instanceType": "t3.medium"
            }
        });

        assert_eq!(
            serde_json::to_string_pretty(&sorted_json(&generated_variable_collection)).unwrap(),
            serde_json::to_string_pretty(&sorted_json(&expected_variable_collection)).unwrap()
        );
    }
}
