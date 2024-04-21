use aws_sdk_lambda::primitives::Blob;
use aws_sdk_lambda::types::InvocationType;
use aws_sdk_lambda::Client;
use log::{error, warn};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Serialize, Deserialize)]
struct ApiInfraPayload {
    event: String,
    module: String,
    name: String,
    environment: String,
    deployment_id: String,
    spec: serde_json::value::Value,
    annotations: serde_json::value::Value,
}

pub async fn mutate_infra(
    event: String,
    module: String,
    name: String,
    environment: String,
    deployment_id: String,
    spec: serde_json::value::Value,
    annotations: serde_json::value::Value,
) -> anyhow::Result<String> {
    let payload = ApiInfraPayload {
        event: event.clone(),
        module: module.clone().to_lowercase(), // TODO: Only have access to kind, not the module name (which is assumed to be lowercase of module_name)
        name: name.clone(),
        environment: environment.clone(),
        deployment_id: deployment_id.clone(),
        spec: spec,
        annotations: annotations,
    };

    let shared_config = aws_config::from_env().load().await;
    let region_name = shared_config.region().unwrap();

    let client = Client::new(&shared_config);
    let api_function_name = "infrastructureApi";

    let serialized_payload = serde_json::to_vec(&payload).unwrap();
    let payload_blob = Blob::new(serialized_payload);

    warn!(
        "Invoking {}-job {} in region {} using {} for environment {} with payload: {:?}",
        event, deployment_id, region_name, api_function_name, environment, payload
    );
    println!(
        "Invoking {}-job {} in region {} using {} for environment {} with payload: {:?}",
        event, deployment_id, region_name, api_function_name, environment, payload
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
        // Although we get the deployment id, the name and namespace etc is unique within the cluster
        // and patching it here causes a race condition, so we should not do it here

        let body = parsed_json
            .get("body")
            .expect("body not found")
            .as_str()
            .expect("body not a string");
        let body_json: Value = serde_json::from_str(body).expect("body not valid JSON");
        let deployment_id = body_json
            .get("deployment_id")
            .expect("deployment_id not found")
            .as_str()
            .expect("deployment_id not a string");
        warn!("Deployment ID: {:?}", deployment_id);
        Ok(deployment_id.to_string())
    } else {
        Err(anyhow::anyhow!("Payload missing from Lambda response"))
    }
}
