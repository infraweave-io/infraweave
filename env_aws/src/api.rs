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

    warn!(
        "Invoking generic job in region {} with payload: {:?}",
        &payload.clone(),
        region_name
    );
    println!(
        "Invoking generic job in region {} with payload: {:?}",
        &payload.clone(),
        region_name
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
        // Although we get the deployment id, the name and namespace etc is unique within the cluster
        // and patching it here causes a race condition, so we should not do it here

        Ok(parsed_json)
    } else {
        Err(anyhow::anyhow!("Payload missing from Lambda response"))
    }
}
