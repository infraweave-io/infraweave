use aws_sdk_lambda::primitives::Blob;
use aws_sdk_lambda::types::InvocationType;
use aws_sdk_lambda::Client;
use log::{error, warn};
use serde_json::Value;

pub async fn run_lambda(payload: Value) -> anyhow::Result<Value> {
    let shared_config = aws_config::from_env().load().await;
    // let region_name = shared_config.region().unwrap();

    let client = Client::new(&shared_config);
    let api_function_name = "infraweave_api";
    let region_name = shared_config.region().unwrap();

    let serialized_payload =
        serde_json::to_vec(&payload).expect(&format!("Failed to serialize payload: {}", payload));

    let payload_blob = Blob::new(serialized_payload);

    let sanitized_payload = serde_json::to_string(&sanitize_payload_for_logging(payload.clone())).unwrap();
    warn!(
        "Invoking generic job in region {:?} with payload: {}",
        region_name,
        sanitized_payload,
    );
    println!(
        "Invoking generic job in region {:?} with payload: {}",
        region_name,
        sanitized_payload,
    );

    let request = client
        .invoke()
        .function_name(api_function_name)
        .invocation_type(InvocationType::RequestResponse)
        .payload(payload_blob);

    let response = match request.send().await {
        Ok(response) => response,
        Err(e) => {
            error!("Failed to invoke Lambda: {}", e);
            let error_message = format!("Failed to invoke Lambda: {}", e);
            return Err(anyhow::anyhow!(error_message));
        }
    };

    if let Some(blob) = response.payload {
        let bytes = blob.into_inner(); // Gets the Vec<u8>
        let response_string = String::from_utf8(bytes).expect("response not valid UTF-8");
        warn!("Lambda response: {:?}", response_string);
        let parsed_json: Value =
            serde_json::from_str(&response_string).expect("response not valid JSON");
        warn!("Parsed JSON: {:?}", parsed_json);
        println!("Parsed JSON: {:?}", parsed_json);

        if parsed_json.get("errorType").is_some() {
            return Err(anyhow::anyhow!(
                "Error in Lambda response: {}",
                parsed_json.get("errorType").unwrap()
            ));
        }
        Ok(parsed_json)
    } else {
        Err(anyhow::anyhow!("Payload missing from Lambda response"))
    }
}

fn sanitize_payload_for_logging(payload: Value) -> Value {
    let mut payload = payload;

    if let Some(event) = payload.get("event") {
        if let Some(event_str) = event.as_str() {
            // if event is upload_file_base64, replace the base64_content with a placeholder
            if event_str == "upload_file_base64" {
                if let Some(data) = payload.get_mut("data") {
                    if let Some(data_obj) = data.as_object_mut() {
                        if let Some(base64_content) = data_obj.get_mut("base64_content") {
                            *base64_content = Value::String("<SANITIZED_BASE64_CONTENT_HERE>".to_string());
                        }
                    }
                }
            }
        }
    }

    payload
}
