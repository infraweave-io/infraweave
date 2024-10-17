use aws_config::meta::region::RegionProviderChain;

use crate::api_module::get_latest_module_version;
use crate::api_stack::get_latest_stack_version;
use crate::get_module_download_url;

pub async fn get_region() -> String {
    let region: String = match RegionProviderChain::default_provider().region().await {
        Some(d) => d.as_ref().to_string(),
        None => "eu-central-1".to_string(),
    };
    region
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

pub async fn compare_latest_version(
    module: &String,
    version: &String,
    environment: &String,
    module_type: ModuleType,
) -> Result<Option<env_defs::ModuleResp>, anyhow::Error> {
    let fetch_module: Result<env_defs::ModuleResp, anyhow::Error> = match module_type {
        ModuleType::Module => get_latest_module_version(module, environment).await,
        ModuleType::Stack => get_latest_stack_version(module, environment).await,
    };


    let entity = if module_type == ModuleType::Module {
        "Module"
    } else {
        "Stack"
    };

    if let Ok(latest_module) = fetch_module {
        let manifest_version = env_utils::semver_parse(&version).unwrap();
        let latest_version = env_utils::semver_parse(&latest_module.version).unwrap();

        if manifest_version == latest_version {
            println!(
                "{} version {} already exists in environment {}",
                entity, manifest_version, environment
            );
            return Err(anyhow::anyhow!(
                "{} version {} already exists in environment {}",
                entity,
                manifest_version,
                environment
            ));
        } else if !(manifest_version > latest_version) {
            return Err(anyhow::anyhow!(
                "{} version {} is older than the latest version {} in environment {}",
                entity,
                manifest_version,
                latest_version,
                environment
            ));
        } else {
            println!(
                "{} version {} is confirmed to be the newest version",
                entity, manifest_version
            );
            return Ok(Some(latest_module));
        }
    }

    println!(
        "No {} found with name: {} and environment: {}",
        entity.to_lowercase(), &module, &environment
    );
    println!("Creating new {} version", entity.to_lowercase());
    Ok(None)
}


pub async fn download_module_to_vec(
    s3_key: &String,
) -> Vec<u8> {
    println!("Downloading module from {}...", s3_key);

    let url = match get_module_download_url(s3_key).await {
        Ok(url) => url,
        Err(e) => {
            panic!("Error: {:?}", e);
        }
    };

    let zip_vec = match env_utils::download_zip_to_vec(&url).await {
        Ok(content) => {
            println!("Downloaded module");
            content
        }
        Err(e) => {
            panic!("Error: {:?}", e);
        }
    };

    zip_vec
}