use env_defs::ApiInfraPayload;

pub async fn mutate_infra(payload: ApiInfraPayload) -> anyhow::Result<String> {
    // let shared_config = aws_config::from_env().load().await;
    // let region_name = shared_config.region().unwrap();

    // let client = Client::new(&shared_config);
    // let api_function_name = "infrastructureApi";

    // let serialized_payload = serde_json::to_vec(&payload).unwrap();
    // let payload_blob = Blob::new(serialized_payload);

    // warn!("Invoking {}-job {} in region {} using {} for environment {} with payload: {:?}", event, deployment_id, region_name, api_function_name, environment, payload);

    // let request = client.invoke()
    //     .function_name(api_function_name)
    //     .invocation_type(InvocationType::RequestResponse)
    //     .payload(payload_blob);

    let function_key = "...uHp8jmw=="; // Typically found in your Azure Function's settings
    let function_url = "https://example....azurewebsites.net/api/api_infra";

    let client = reqwest::Client::new();
    let res = client
        .post(function_url)
        .header("x-functions-key", function_key)
        .json(&payload)
        .send()
        .await?;

    if res.status().is_success() {
        println!(
            "Function invoked successfully. Response: {:?}",
            res.text().await?
        );
    } else {
        eprintln!(
            "Failed to invoke function. Status: {}, text: {:?}",
            res.status(),
            res.text().await?
        );
    }

    Ok("".to_string())
    // let response = match client.send().await {
    //     Ok(response) => response,
    //     Err(e) => {
    //         error!("Failed to invoke Lambda: {}", e);
    //         let error_message = format!("Failed to invoke Lambda: {}", e);
    //         return Err(anyhow::anyhow!(error_message));
    //     },
    // };

    // if let Some(blob) = response.payload {
    //     let bytes = blob.into_inner(); // Gets the Vec<u8>
    //     let response_string = String::from_utf8(bytes).expect("response not valid UTF-8");
    //     warn!("Lambda response: {:?}", response_string);
    //     let parsed_json: Value = serde_json::from_str(&response_string).expect("response not valid JSON");
    //     warn!("Parsed JSON: {:?}", parsed_json);
    //     // Although we get the deployment id, the name and namespace etc is unique within the cluster
    //     // and patching it here causes a race condition, so we should not do it here

    //     let body = parsed_json.get("body").expect("body not found").as_str().expect("body not a string");
    //     let body_json: Value = serde_json::from_str(body).expect("body not valid JSON");
    //     let deployment_id = body_json.get("deployment_id").expect("deployment_id not found").as_str().expect("deployment_id not a string");
    //     warn!("Deployment ID: {:?}", deployment_id);
    //     Ok(deployment_id.to_string())
    // } else {
    //     Err(anyhow::anyhow!("Payload missing from Lambda response"))
    // }
}
