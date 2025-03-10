pub async fn get_securestring_aws(param_name: &str) -> Result<String, anyhow::Error> {
    let config = aws_config::load_from_env().await;
    let client = aws_sdk_ssm::Client::new(&config);

    let resp = client
        .get_parameter()
        .name(param_name)
        .with_decryption(true)
        .send()
        .await?;

    if let Some(parameter) = resp.parameter {
        if let Some(secret) = parameter.value {
            Ok(secret)
        } else {
            Err(anyhow::anyhow!("Parameter {} has no value", param_name))
        }
    } else {
        Err(anyhow::anyhow!("Parameter {} not found", param_name))
    }
}
