use anyhow::Result;
use env_aws::get_latest_module_version_query;
use env_defs::{EnvironmentResp, ModuleManifest, ModuleResp, ModuleVersionDiff, TfOutput, TfVariable};
use env_utils::{
    generate_module_example_deployment, get_outputs_from_tf_files, get_timestamp, get_variables_from_tf_files, merge_json_dicts, read_tf_directory, semver_parse, validate_module_schema, validate_tf_backend_not_set, zero_pad_semver
};
use log::error;
use serde_json::Value;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::{interface::{AwsCloudHandler, CloudHandler}, logic::{common::handler, utils::ModuleType}};

fn get_identifier(deployment_id: &str, track: &str) -> String {
    format!("{}::{}", track, deployment_id)
}

pub async fn publish_module(
    manifest_path: &String,
    track: &String,
) -> anyhow::Result<(), anyhow::Error> {
    let module_yaml_path = Path::new(manifest_path).join("module.yaml");
    let manifest =
        std::fs::read_to_string(&module_yaml_path).expect("Failed to read module manifest file");

    let module_yaml =
        serde_yaml::from_str::<ModuleManifest>(&manifest).expect("Failed to parse module manifest");

    let zip_file = env_utils::get_zip_file(&Path::new(manifest_path), &module_yaml_path).await?;
    // Encode the zip file content to Base64
    let zip_base64 = base64::encode(&zip_file);

    let tf_content = read_tf_directory(&Path::new(manifest_path)).unwrap(); // Get all .tf-files concatenated into a single string

    match validate_tf_backend_not_set(&tf_content) {
        std::result::Result::Ok(_) => (),
        Err(error) => {
            println!("{}", error);
            std::process::exit(1);
        }
    }

    match validate_module_schema(&manifest) {
        std::result::Result::Ok(_) => (),
        Err(error) => {
            println!("{}", error);
            std::process::exit(1);
        }
    }

    let tf_variables = get_variables_from_tf_files(&tf_content).unwrap();
    let tf_outputs = get_outputs_from_tf_files(&tf_content).unwrap();

    let module = module_yaml.metadata.name.clone();
    let version = module_yaml.spec.version.clone();

    let manifest_version = env_utils::semver_parse(&version).unwrap();
    println!("Manifest version: {}. Checking if this is the newest", manifest_version);
    match &manifest_version.pre.to_string() == track {
        true => {
            if track == "dev" || track == "alpha" || track == "beta" || track == "rc" {
                println!("Pushing to {} track", track);
            } else if track == "stable" {
                println!("Pushing to stable track should not specify pre-release version, only major.minor.patch");
                std::process::exit(1);
            } else {
                println!("Invalid track \"{}\", allowed tracks: rc, beta, alpha, dev, stable", track);
                std::process::exit(1);
            }
        },
        false => {
            if &manifest_version.pre.to_string() == "" && track == "stable" {
                println!("Pushing to stable track");
            } else {
                println!(
                    "Track \"{}\" must match be one of the allowed tracks: \"rc\", \"beta\", \"alpha\", \"dev\", \"stable\". And match the pre-release version \"{}\"",
                    track,
                    manifest_version.pre
                );
                std::process::exit(1);
            }
        }
    };

    println!("Publishing module: {}, version \"{}.{}.{}\", pre-release/track \"{}\", build \"{}\"", module, manifest_version.major, manifest_version.minor, manifest_version.patch, manifest_version.pre, manifest_version.build);

    let latest_version: Option<ModuleResp>  = match compare_latest_version(&module, &version, &track, ModuleType::Module).await {
        Ok(existing_version) => existing_version, // Returns existing module if newer, otherwise it's the first module version to be published
        Err(error) => {
            println!("{}", error);
            std::process::exit(1); // If the module version already exists and is older, exit
        }
    };

    let version_diff = match latest_version { // TODO break out to function
        Some(previous_existing_module) => {
            let current_version_module_hcl_str = &tf_content;

            // Download the previous version of the module and get hcl content
            let previous_version_s3_key = &previous_existing_module.s3_key;
            let previous_version_module_zip = download_module_to_vec(previous_version_s3_key).await;
        
            // Extract all hcl blocks from the zip file
            let previous_version_module_hcl_str = match env_utils::read_tf_from_zip(&previous_version_module_zip){
                Ok(hcl_str) => hcl_str,
                Err(error) => {
                    println!("{}", error);
                    std::process::exit(1);
                }
            };

            // Compare with existing hcl blocks in current version
            let (additions, changes, deletions) = env_utils::diff_modules(&previous_version_module_hcl_str, &current_version_module_hcl_str);

            Some(ModuleVersionDiff {
                added: additions,
                changed: changes,
                removed: deletions,
                previous_version: previous_existing_module.version.clone(),
            })
        },
        _ => {
            None
        }
    };

    let module = ModuleResp {
        track: track.clone(),
        track_version: format!(
            "{}#{}",
            track.clone(),
            zero_pad_semver(module_yaml.spec.version.as_str(), 3).unwrap()
        ),
        version: module_yaml.spec.version.clone(),
        timestamp: get_timestamp(),
        module: module_yaml.metadata.name.clone(),
        module_name: module_yaml.spec.module_name.clone(),
        description: module_yaml.spec.description.clone(),
        reference: module_yaml.spec.reference.clone(),
        manifest: module_yaml.clone(),
        tf_variables: tf_variables,
        tf_outputs: tf_outputs,
        s3_key: format!(
            "{}/{}-{}.zip",
            &module_yaml.metadata.name, &module_yaml.metadata.name, &module_yaml.spec.version
        ), // s3_key -> "{module}/{module}-{version}.zip"
        stack_data: None,
        version_diff: version_diff,
    };

    upload_module(&module, &zip_base64, track).await
}


pub async fn upload_module(
    module: &ModuleResp,
    zip_base64: &String,
    track: &String,
) -> anyhow::Result<(), anyhow::Error> {
    let payload = serde_json::json!({
        "event": "upload_file_base64",
        "data":
        {
            "key": &module.s3_key,
            "bucket_name": "modules",
            "base64_content": &zip_base64
        }

    });
    match handler().run_function(&payload).await {
        Ok(_) => {
            println!("Successfully uploaded module zip file to storage");
        }
        Err(error) => {
            println!("{}", error);
            std::process::exit(1);
        }
    }

    match insert_module(&module).await {
        Ok(_) => {
            println!("Successfully published module {}", module.module);
        }
        Err(error) => {
            println!("{}", error);
            std::process::exit(1);
        }
    }

    // trigger_docs_generator()

    println!(
        "Publishing version {} of module {}",
        module.version, module.module
    );

    Ok(())
}


pub async fn insert_module(module: &ModuleResp) -> anyhow::Result<String> {
    let MODULE_TABLE_NAME = "Modules-eu-central-1-dev";

    let mut transaction_items = vec![];

    let id: String = format!(
        "MODULE#{}",
        get_identifier(&module.module, &module.track)
    );

    // -------------------------
    // Module metadata
    // -------------------------
    let mut module_payload = serde_json::to_value(serde_json::json!({
        "PK": id.clone(),
        "SK": format!("VERSION#{}", zero_pad_semver(&module.version, 3).unwrap()),
    }))
    .unwrap();

    let module_value = serde_json::to_value(&module).unwrap();
    merge_json_dicts(&mut module_payload, &module_value);

    transaction_items.push(serde_json::json!({
        "Put": {
            "TableName": MODULE_TABLE_NAME,
            "Item": module_payload
        }
    }));

    // -------------------------
    // Latest module version
    // -------------------------
    // It is inserted as a MODULE (above) but LATEST-prefix is used to differentiate stack and module (to reduce maintenance)
    let latest_pk = if module.stack_data.is_some() {
        "LATEST_STACK"
    } else {
        "LATEST_MODULE"
    };
    let mut latest_module_payload = serde_json::to_value(serde_json::json!({
        "PK": latest_pk,
        "SK": id.clone(),
    }))
    .unwrap();

    // Use the same module metadata to the latest module version
    merge_json_dicts(&mut latest_module_payload, &module_value);

    transaction_items.push(serde_json::json!({
        "Put": {
            "TableName": MODULE_TABLE_NAME,
            "Item": latest_module_payload
        }
    }));

    // -------------------------
    // Execute the Transaction
    // -------------------------

    let payload = serde_json::json!({
        "event": "transact_write",
        "items": transaction_items,
    });
    match handler().run_function(&payload).await {
        Ok(response) => Ok(response.payload.to_string()),
        Err(e) => Err(e),
    }
}

pub async fn compare_latest_version(
    module: &String,
    version: &String,
    track: &String,
    module_type: ModuleType,
) -> Result<Option<ModuleResp>, anyhow::Error> {
    let fetch_module: Result<Option<ModuleResp>, anyhow::Error> = match module_type {
        ModuleType::Module => handler().get_latest_module_version(module, track).await,
        ModuleType::Stack => handler().get_latest_stack_version(module, track).await,
    };

    let entity = if module_type == ModuleType::Module {
        "Module"
    } else {
        "Stack"
    };

    match fetch_module {
        Ok(fetch_module) => {
            if let Some(latest_module) = fetch_module {
                let manifest_version = env_utils::semver_parse(&version).unwrap();
                let latest_version = env_utils::semver_parse(&latest_module.version).unwrap();

                if manifest_version == latest_version {
                    return Err(anyhow::anyhow!(
                        "{} version {} already exists in track {}",
                        entity,
                        manifest_version,
                        track
                    ));
                } else if !(manifest_version > latest_version) {
                    return Err(anyhow::anyhow!(
                        "{} version {} is older than the latest version {} in track {}",
                        entity,
                        manifest_version,
                        latest_version,
                        track
                    ));
                } else {
                    println!(
                        "{} version {} is confirmed to be the newest version",
                        entity, manifest_version
                    );
                    return Ok(Some(latest_module));
                }
            } else {
                println!(
                    "No existing {} version found in track {}, this is the first version",
                    entity, track
                );
                return Ok(None);
            }
        },
        Err(e) => {
            println!("An error occurred: {:?}", e);
            return Err(e);
        }
    };
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

pub async fn get_module_download_url(key: &String) -> Result<String, anyhow::Error> {
    let url = match handler().generate_presigned_url(key).await {
        Ok(response) => response,
        Err(e) => {
            return Err(anyhow::anyhow!("Failed to read db: {}", e));
        }
    };
    Ok(url)
}

pub async fn list_modules(track: &str) -> Result<Vec<ModuleResp>, anyhow::Error> {
    handler().get_all_latest_module(track).await
}

pub async fn get_all_module_versions(module: &str, track: &str) -> Result<Vec<ModuleResp>, anyhow::Error> {
    handler().get_all_module_versions(module, track).await
}

pub async fn get_module_version(module: &str, track: &str, version: &str) -> Result<Option<ModuleResp>, anyhow::Error> {
    handler().get_module_version(module, track, version).await
}

pub async fn get_latest_module_version(module: &str, track: &str) -> Result<Option<ModuleResp>, anyhow::Error> {
    handler().get_latest_module_version(module, track).await
}

pub async fn precheck_module(manifest_path: &String, track: &String) -> anyhow::Result<(), anyhow::Error> {
    let module_yaml_path = Path::new(manifest_path).join("module.yaml");
    let manifest =
        std::fs::read_to_string(&module_yaml_path).expect("Failed to read module manifest file");

    let module_yaml =
        serde_yaml::from_str::<ModuleManifest>(&manifest).expect("Failed to parse module manifest");

    // println!("Prechecking module: {}", module_yaml.metadata.name);
    // println!("Full module: {:#?}", module_yaml);
    // println!("Examples: {:#?}", module_yaml.spec.examples);

    let module_spec = &module_yaml.spec.clone();
    let examples = &module_spec.examples;

    if let Some(examples) = examples {
        for example in examples {
            let example_claim = generate_module_example_deployment(module_spec, &example);
            let claim_str = serde_yaml::to_string(&example_claim).unwrap();
            println!("{}", claim_str);
        }
    } else {
        println!("No examples found in module.yaml");
    }

    Ok(())
}