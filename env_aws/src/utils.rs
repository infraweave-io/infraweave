use aws_config::{meta::region::RegionProviderChain, Region};

pub async fn get_region() -> String {
    let region_provider = RegionProviderChain::default_provider().or_default_provider();
    let region = region_provider.region().await.expect("Failed to load region");
    region.to_string()
}

pub async fn get_ssm_parameter(
    client: &aws_sdk_ssm::Client,
    parameter_name: &str,
    decrypt: bool,
) -> Result<String, String> {
    // Fetch the parameter from SSM
    let response = client
        .get_parameter()
        .name(parameter_name)
        .with_decryption(decrypt)
        .send()
        .await
        .map_err(|e| format!("Failed to get parameter '{}': {}", parameter_name, e))?;

    if let Some(parameter) = response.parameter {
        if let Some(value) = parameter.value {
            return Ok(value);
        }
    }

    Err(format!(
        "Parameter '{}' not found, did you bootstrap?",
        parameter_name
    ))
}

#[derive(PartialEq)]
pub enum ModuleType {
    Module,
    Stack,
}
