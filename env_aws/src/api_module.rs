use anyhow::Result;
use aws_config::meta::region::RegionProviderChain;
use aws_sdk_lambda::primitives::Blob;
use aws_sdk_lambda::Client;
use chrono::{Local, TimeZone};
use env_defs::{EnvironmentResp, ModuleManifest, ModuleResp, ModuleVersionDiff, TfOutput, TfVariable};
use env_utils::{
    generate_module_example_deployment, get_outputs_from_tf_files, get_variables_from_tf_files, merge_json_dicts, read_tf_directory, semver_parse, validate_module_schema, validate_tf_backend_not_set, zero_pad_semver
};
use log::error;
use serde_json::Value;
use std::path::Path;

use crate::{api::run_lambda, compare_latest_version, utils::{download_module_to_vec, ModuleType}};

use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
struct ApiPublishModuleLambdaPayload {
    event: String,
    manifest: String,
    track: String,
    description: String,
    reference: String,
    zip_file_base64: String,
    tf_variables: Vec<TfVariable>,
    tf_outputs: Vec<TfOutput>,
}

#[derive(Debug, Serialize, Deserialize)]
struct ApiGetModuleLambdaPayload {
    deployment_id: String,
    query: Value,
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

    // let tf_variables =
    //     env_utils::parse_hcl_file_to_json_string(&Path::new(manifest_path).join("variables.tf"))
    //         .unwrap_or("{}".to_string());


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
        None => {
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
        timestamp: Local::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
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
    match upload_file_base64(&module.s3_key, &zip_base64).await {
        Ok(_) => {
            println!("Successfully uploaded module zip file to S3");
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


pub async fn list_module(track: &str) -> Result<Vec<ModuleResp>, anyhow::Error> {
    _list_module("LATEST_MODULE", track).await
}

pub async fn _list_module(pk: &str, track: &str) -> Result<Vec<ModuleResp>, anyhow::Error> {
    let response = read_db(serde_json::json!({
        "KeyConditionExpression": "PK = :latest",
        "ExpressionAttributeValues": {":latest": pk},
    }))
    .await?;

    let items = response.get("Items").expect("Items not found");

    if let Some(modules) = items.as_array() {
        let modules_string =
            serde_json::to_string(modules).expect("Failed to convert modules to string");
        let modules: Vec<ModuleResp> =
            serde_json::from_str(&modules_string).expect("Failed to parse inner JSON string");

        println!(
            "{:<20} {:<20} {:<20} {:<15} {:<10} {:<30}",
            "Module", "ModuleName", "Version", "Track", "Ref", "Description"
        );
        for entry in &modules {
            println!(
                "{:<20} {:<20} {:<20} {:<15} {:<10} {:<30}",
                entry.module,
                entry.module_name,
                entry.version,
                entry.track,
                entry.reference,
                entry.description
            );
        }
        return Ok(modules);
    } else {
        println!("No payload in response");
    }
    Ok([].to_vec())
}

pub async fn get_all_module_versions(module: &str, track: &str) -> Result<Vec<ModuleResp>, anyhow::Error> {
    let id: String = format!(
        "MODULE#{}",
        get_identifier(&module, &track)
    );
    let sk = "VERSION#";
    let response = read_db(serde_json::json!({
        "KeyConditionExpression": "PK = :module AND begins_with(SK, :sk)",
        "ExpressionAttributeValues": {":module": id, ":sk": sk},
        "ScanIndexForward": false,
    }))
    .await?;

    let items = response.get("Items").expect("Items not found");

    if let Some(modules) = items.as_array() {
        let modules_string =
            serde_json::to_string(modules).expect("Failed to convert modules to string");
        let modules: Vec<ModuleResp> =
            serde_json::from_str(&modules_string).expect("Failed to parse inner JSON string");

        println!(
            "{:<20} {:<20} {:<20} {:<15} {:<10} {:<30}",
            "Module", "ModuleName", "Version", "Track", "Ref", "Description"
        );
        for entry in &modules {
            println!(
                "{:<20} {:<20} {:<20} {:<15} {:<10} {:<30}",
                entry.module,
                entry.module_name,
                entry.version,
                entry.track,
                entry.reference,
                entry.description
            );
        }
        return Ok(modules);
    } else {
        println!("No payload in response");
    }
    Ok([].to_vec())
}

pub async fn get_module_download_url(key: &String) -> Result<String, anyhow::Error> {
    let payload = serde_json::json!({
        "event": "generate_presigned_url",
        "data":{
            "key": key,
            "bucket_name": "modules",
            "expires_in": 60,
        }
    });

    let response = match run_lambda(payload).await {
        Ok(response) => response,
        Err(e) => {
            error!("Failed to read db: {}", e);
            println!("Failed to read db: {}", e);
            // Ok(vec![])
            return Err(anyhow::anyhow!("Failed to read db: {}", e));
        }
    };

    let url = response
        .get("url")
        .expect("Presigned url not found")
        .as_str()
        .unwrap()
        .to_string();

    Ok(url)
}

pub async fn list_environments() -> Result<Vec<EnvironmentResp>, anyhow::Error> {
    // let region_provider = RegionProviderChain::default_provider().or_else("us-east-1");
    // let shared_config = aws_config::from_env().region(region_provider).load().await;
    // let client = Client::new(&shared_config);

    // let function_name = "moduleApi";
    // let payload = serde_json::json!({
    //     "event": "list_environments"
    // });

    // let response = client
    //     .invoke()
    //     .function_name(function_name)
    //     .payload(Blob::new(serde_json::to_vec(&payload).unwrap()))
    //     .send()
    //     .await?;

    // if let Some(blob) = response.payload {
    //     let payload_bytes = blob.into_inner();
    //     let payload_str =
    //         String::from_utf8(payload_bytes).expect("Failed to convert payload to String");

    //     let parsed: serde_json::Value =
    //         serde_json::from_str(&payload_str).expect("Failed to parse string to JSON Value");

    //     if let Some(inner_json_str) = parsed.as_str() {
    //         let environments: Vec<EnvironmentResp> =
    //             serde_json::from_str(inner_json_str).expect("Failed to parse inner JSON string");
    //         let datetime_result = Local.timestamp_opt(0, 0);

    //         if let chrono::LocalResult::Single(datetime) = datetime_result {
    //             let utc_offset = datetime.format("%:z").to_string();
    //             let simplified_offset = simplify_utc_offset(&utc_offset);

    //             println!(
    //                 "{:<25} {:<15}",
    //                 "Environments",
    //                 format!("LastActivity ({})", simplified_offset)
    //             );

    //             for entry in environments {
    //                 let entry_datetime_result = Local.timestamp_opt(entry.last_activity_epoch, 0);

    //                 if let chrono::LocalResult::Single(entry_datetime) = entry_datetime_result {
    //                     let date_string = entry_datetime.format("%Y-%m-%d %H:%M:%S").to_string();
    //                     println!("{:<25} {:<15}", entry.environment, date_string);
    //                 } else {
    //                     println!(
    //                         "Failed to convert last activity timestamp for environment: {}",
    //                         entry.environment
    //                     );
    //                 }
    //             }
    //         } else {
    //             println!("Failed to obtain UTC offset");
    //         }
    //     }
    // } else {
    //     println!("No payload in response");
    // }

    Ok([].to_vec())
}

pub async fn get_module_version(module: &String, track: &String, version: &String) -> anyhow::Result<ModuleResp> {
    let id: String = format!(
        "MODULE#{}",
        get_identifier(&module, &track)
    );
    let version_id = format!("VERSION#{}", zero_pad_semver(&version, 3).unwrap());
    let response = read_db(serde_json::json!({
        "KeyConditionExpression": "PK = :module AND SK = :sk",
        "ExpressionAttributeValues": {":module": id, ":sk": version_id},
        "Limit": 1,
    }))
    .await?;

    let items = response.get("Items").expect("Items not found");

    let modules_string = serde_json::to_string(items).expect("Failed to convert modules to string");
    let modules: Vec<ModuleResp> =
        serde_json::from_str(&modules_string).expect("Failed to parse inner JSON string");

    if modules.len() == 0 {
        println!(
            "No module found with module: {} and version: {}",
            module, version
        );
        return Err(anyhow::anyhow!(
            "No module found with module: {} and version: {}",
            module,
            version
        ));
    }

    let module = &modules[0];

    let yaml_string = serde_yaml::to_string(&module.manifest).unwrap();
    println!("Information\n------------");
    println!("Module: {}", module.module);
    println!("Version: {}", module.version);
    println!("Description: {}", module.description);
    println!("Reference: {}", module.reference);
    println!("\n");

    println!("{}", yaml_string);
    return Ok(module.clone());
}

pub async fn get_latest_module_version(
    module: &String,
    track: &String,
) -> anyhow::Result<ModuleResp> {
    _get_latest_module_version("LATEST_MODULE", module, track).await
}

pub async fn _get_latest_module_version(
    pk: &str,
    module: &String,
    track: &String,
) -> anyhow::Result<ModuleResp> {
    let sk: String = format!(
        "MODULE#{}",
        get_identifier(&module, &track)
    );
    let response = read_db(serde_json::json!({
        "KeyConditionExpression": "PK = :latest AND SK = :sk",
        "ExpressionAttributeValues": {":latest": pk, ":sk": sk},
        "Limit": 1,
    }))
    .await?;

    let items = response.get("Items").expect("Items not found");

    let modules_string = serde_json::to_string(items).expect("Failed to convert modules to string");
    let modules: Vec<ModuleResp> =
        serde_json::from_str(&modules_string).expect("Failed to parse inner JSON string");

    if modules.len() == 0 {
        println!(
            "No module found with module: {} and track: {}",
            module, track
        );
        return Err(anyhow::anyhow!(
            "No module found with module: {} and track: {}",
            module,
            track
        ));
    }

    let module = &modules[0];

    let yaml_string = serde_yaml::to_string(&module.manifest).unwrap();
    println!("Information\n------------");
    println!("Module: {}", module.module);
    println!("Version: {}", module.version);
    println!("Description: {}", module.description);
    println!("Reference: {}", module.reference);
    println!("\n");

    println!("{}", yaml_string);
    return Ok(module.clone());
}

fn simplify_utc_offset(offset: &str) -> String {
    let parts: Vec<&str> = offset.split(':').collect();
    if parts.len() == 2 {
        let hours = parts[0];
        // Assuming we don't need to deal with minutes for simplification
        format!("UTC{}", hours)
    } else {
        String::from("UTC")
    }
}

async fn read_db(query: Value) -> Result<Value, anyhow::Error> {
    let payload = ApiGetModuleLambdaPayload {
        deployment_id: "".to_string(),
        query: query,
    };

    let payload = serde_json::json!({
        "event": "read_db",
        "table": "modules",
        "data": payload
    });

    let response = match run_lambda(payload).await {
        Ok(response) => response,
        Err(e) => {
            error!("Failed to read db: {}", e);
            println!("Failed to read db: {}", e);
            return Err(anyhow::anyhow!("Failed to read db: {}", e));
        }
    };

    Ok(response)
}

async fn upload_file_base64(key: &String, base64_content: &String) -> Result<Value, anyhow::Error> {
    let payload = serde_json::json!({
        "event": "upload_file_base64",
        "data":
        {
            "key": key,
            "bucket_name": "modules",
            "base64_content": base64_content
        }

    });

    let response = match run_lambda(payload).await {
        Ok(response) => response,
        Err(e) => {
            error!("Failed to read db: {}", e);
            println!("Failed to read db: {}", e);
            return Err(anyhow::anyhow!("Failed to read db: {}", e));
        }
    };

    Ok(response)
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

    println!("Invoking Lambda with payload: {}", payload);

    match run_lambda(payload).await {
        Ok(_) => Ok("".to_string()),
        Err(e) => {
            error!("Failed to insert policy: {}", e);
            Err(anyhow::anyhow!("Failed to insert policy: {}", e))
        }
    }
}


fn get_identifier(deployment_id: &str, track: &str) -> String {
    format!("{}::{}", track, deployment_id)
}
