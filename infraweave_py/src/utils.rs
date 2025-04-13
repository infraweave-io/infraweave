use env_utils::to_camel_case;
use log::debug;
use serde_json::Value;

pub fn get_variable_mapping(is_stack: bool, variables: &Value) -> Value {
    let variables_obj = variables.as_object().expect("variables is not an object");

    let mapped = if is_stack {
        // For stacks, separate keys into a nested mapping when they contain "__"
        variables_obj
            .iter()
            .fold(serde_json::Map::new(), |mut acc, (k, v)| {
                if let Some((component, property_key)) = k.split_once("__") {
                    let camel_property_key = to_camel_case(property_key);
                    let component_entry = acc
                        .entry(component.to_string())
                        .or_insert_with(|| serde_json::json!({}));
                    if let serde_json::Value::Object(ref mut nested_map) = component_entry {
                        nested_map.insert(camel_property_key, v.clone());
                    }
                } else {
                    // Insert non-nested keys with camelCase conversion.
                    acc.insert(to_camel_case(k), v.clone());
                }
                acc
            })
    } else {
        // For modules (non-stack) simply convert all keys to camelCase.
        variables_obj
            .iter()
            .fold(serde_json::Map::new(), |mut acc, (k, v)| {
                debug!("Replacing key {} with {}", k, to_camel_case(k));
                acc.insert(to_camel_case(k), v.clone());
                acc
            })
    };

    serde_json::Value::Object(mapped)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn test_module_variable_case_conversion() {
        let variables = serde_json::json!({
            "bucket_name": "some_bucket_name",
            "enable_acl": false,
            "tags": {
                "tag_environment": "some_value1",
                "tag_name": "some_value2"
            }
        });

        let expected_variables = serde_json::json!({
            "bucketName": "some_bucket_name",
            "enableAcl": false,
            "tags": {
                "tag_environment": "some_value1",
                "tag_name": "some_value2"
            }
        });
        assert_eq!(expected_variables, get_variable_mapping(false, &variables));
    }

    #[test]
    fn test_stack_variable_case_conversion() {
        let variables = serde_json::json!({
            "bucket1__bucket_name": "some_bucket_name",
            "bucket1__enable_acl": false,
            "bucket1__tags": {
                "tag_environment": "some_value1",
                "tag_name": "some_value2"
            }
        });

        let expected_variables = serde_json::json!({
            "bucket1": {
                "bucketName": "some_bucket_name",
                "enableAcl": false,
                "tags": {
                    "tag_environment": "some_value1",
                    "tag_name": "some_value2"
                }
            }
        });
        assert_eq!(expected_variables, get_variable_mapping(true, &variables));
    }
}
