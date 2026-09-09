use serde_json::Value;

pub fn sanitize_payload_for_logging(mut payload: Value) -> Value {
    if payload.get("event").and_then(Value::as_str) == Some("upload_file_base64") {
        if let Some(content) = payload.pointer_mut("/data/base64_content") {
            *content = Value::String("<SANITIZED_BASE64_CONTENT_HERE>".to_string());
        }
    }

    payload
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_sanitize_payload_for_logging() {
        // Arrange
        let payload = serde_json::json!({
            "event": "upload_file_base64",
            "data": {
                "base64_content": "some_base64_encoded_string_here"
            }
        });

        //Act
        let sanitized_payload = sanitize_payload_for_logging(payload);

        //Assert
        assert_eq!(
            sanitized_payload,
            serde_json::json!({
                "event": "upload_file_base64",
                "data": {
                    "base64_content": "<SANITIZED_BASE64_CONTENT_HERE>"
                }
            })
        );
    }

    #[test]
    fn test_sanitize_payload_for_logging_non_upload_event() {
        // Arrange
        let payload = serde_json::json!({
            "event": "some_other_event",
            "data": {
                "base64_content": "some_base64_encoded_string_here"
            }
        });

        //Act
        let sanitized_payload = sanitize_payload_for_logging(payload.clone());

        //Assert
        assert_eq!(sanitized_payload, payload);
    }

    #[test]
    fn test_sanitize_payload_for_logging_no_data_field() {
        // Arrange
        let payload = serde_json::json!({
            "event": "upload_file_base64"
        });

        //Act
        let sanitized_payload = sanitize_payload_for_logging(payload.clone());

        //Assert
        assert_eq!(sanitized_payload, payload);
    }

    #[test]
    fn test_sanitize_payload_for_logging_no_event_field() {
        // Arrange
        let payload = serde_json::json!({
            "data": {
                "base64_content": "some_base64_encoded_string_here"
            }
        });

        //Act
        let sanitized_payload = sanitize_payload_for_logging(payload.clone());

        //Assert
        assert_eq!(sanitized_payload, payload);
    }

    #[test]
    fn test_sanitize_payload_for_logging_no_base64_content() {
        // Arrange
        let payload = serde_json::json!({
            "event": "upload_file_base64",
            "data": {
                "some_other_field": "some_value"
            }
        });

        //Act
        let sanitized_payload = sanitize_payload_for_logging(payload.clone());

        //Assert
        assert_eq!(sanitized_payload, payload);
    }

    #[test]
    fn test_sanitize_payload_for_logging_nested_data() {
        // Arrange
        let payload = serde_json::json!({
            "event": "upload_file_base64",
            "data": {
                "nested_data": {
                    "base64_content": "some_base64_encoded_string_here"
                }
            }
        });

        //Act
        let sanitized_payload = sanitize_payload_for_logging(payload.clone());

        //Assert
        assert_eq!(sanitized_payload, payload);
    }

    #[test]
    fn test_sanitize_payload_for_logging_non_string_event() {
        // Arrange
        let payload = serde_json::json!({
            "event": 12345,
            "data": {
                "base64_content": "some_base64_encoded_string_here"
            }
        });

        //Act
        let sanitized_payload = sanitize_payload_for_logging(payload.clone());

        //Assert
        assert_eq!(sanitized_payload, payload);
    }

    #[test]
    fn test_sanitize_payload_for_logging_non_object_data() {
        // Arrange
        let payload = serde_json::json!({
            "event": "upload_file_base64",
            "data": "some_string_instead_of_object"
        });

        //Act
        let sanitized_payload = sanitize_payload_for_logging(payload.clone());

        //Assert
        assert_eq!(sanitized_payload, payload);
    }

    #[test]
    fn test_sanitize_payload_for_logging_empty_payload() {
        // Arrange
        let payload = serde_json::json!({});

        //Act
        let sanitized_payload = sanitize_payload_for_logging(payload.clone());

        //Assert
        assert_eq!(sanitized_payload, payload);
    }

    #[test]
    fn test_sanitize_payload_for_logging_null_payload() {
        // Arrange
        let payload = serde_json::json!(null);

        //Act
        let sanitized_payload = sanitize_payload_for_logging(payload.clone());

        //Assert
        assert_eq!(sanitized_payload, payload);
    }
}
