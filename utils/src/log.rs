use serde_json::Value;

pub fn sanitize_payload_for_logging(payload: Value) -> Value {
    let mut payload = payload;

    if let Some(event) = payload.get("event") {
        if let Some(event_str) = event.as_str() {
            // if event is upload_file_base64, replace the base64_content with a placeholder
            if event_str == "upload_file_base64" {
                if let Some(data) = payload.get_mut("data") {
                    if let Some(data_obj) = data.as_object_mut() {
                        if let Some(base64_content) = data_obj.get_mut("base64_content") {
                            *base64_content =
                                Value::String("<SANITIZED_BASE64_CONTENT_HERE>".to_string());
                        }
                    }
                }
            }
        }
    }

    payload
}
