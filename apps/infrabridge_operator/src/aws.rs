use aws_sdk_lambda::{Client, Error};
use aws_sdk_lambda::types::InvocationType;
use aws_sdk_lambda::primitives::Blob;
use serde::{Serialize, Deserialize};
use log::{info, error};

#[derive(Debug, Serialize, Deserialize)]
struct LambdaPayload {
    event: String,
    module: String,
    name: String,
    deployment_id: String,
    spec: serde_json::value::Value,
}

pub async fn mutate_infra(event: String, module: String, name: String, deployment_id: String, spec: serde_json::value::Value) -> Result<(), Error> {
    
    let payload = LambdaPayload {
        event: event.clone(),
        module: module,
        name: name,
        deployment_id: deployment_id.clone(),
        spec: spec,
    };
    
    let shared_config = aws_config::from_env().load().await; 
    let region_name = shared_config.region().unwrap();

    let client = Client::new(&shared_config);
    let api_function_name = "infrastructureApi";

    let serialized_payload = serde_json::to_vec(&payload).unwrap();
    let payload_blob = Blob::new(serialized_payload);

    info!("Invoking {}-job {} in region {} using {} with payload: {:?}", event, deployment_id, region_name, api_function_name, payload);

    let request = client.invoke()
        .function_name(api_function_name)
        .invocation_type(InvocationType::RequestResponse)
        .payload(payload_blob);

    let response = match request.send().await {
        Ok(response) => response,
        Err(e) => {
            error!("Failed to invoke Lambda: {}", e);
            return Err(e.into());
        },
    };

    if let Some(blob) = response.payload {
        let bytes = blob.into_inner(); // Gets the Vec<u8>
        let response_string = String::from_utf8(bytes).expect("response not valid UTF-8");
        info!("Lambda response: {:?}", response_string);
    }

    Ok(())
}
