use aws_sdk_lambda::{Client, Error}; // TO REMOVE
use aws_config::meta::region::RegionProviderChain; // TO REMOVE
use aws_sdk_lambda::primitives::Blob; // TO REMOVE
use serde_json::json;
use anyhow::Result;

use log::{info, error, LevelFilter};
use chrono::{TimeZone, Local};

use env_defs::{ModuleResp, EnvironmentResp};
use crate::environment::run_function;

pub async fn publish_module(manifest_path: &String, environment: &String, description: &String, reference: &String) -> Result<()> {
    let manifest = std::fs::read_to_string(manifest_path)
        .expect("Failed to read module manifest file");

    let payload = json!({
        "event": "insert",
        "manifest": manifest,
        "environment": environment,
        "description": description,
        "reference": reference,
    });

    run_function(&"api_module".to_string(), payload).await.unwrap();

    Ok(())
}



pub async fn list_module(environment: &String) -> Result<Vec<ModuleResp>, anyhow::Error> {
    let function_name = "api_module";
    let payload = serde_json::json!({
        "event": "list_latest",
        "environment": environment
    });

    if let Ok(response_json) = run_function(function_name, payload).await {

        // Check if response_json is a string that needs to be parsed as JSON.
        if let serde_json::Value::String(encoded_array) = &response_json {
            // The string is double-encoded JSON; parse it to get the array.
            let modules_array: serde_json::Value = serde_json::from_str(encoded_array)
                .expect("Failed to parse double-encoded JSON");

            if let serde_json::Value::Array(modules) = modules_array {
                println!("{:<20} {:<20} {:<10} {:<15} {:<10} {:<30}", "Module", "ModuleName", "Version", "Environment", "Ref", "Description");
                for module in &modules {
                    // println!("{:?}", module);
                    match serde_json::from_value::<ModuleResp>(module.clone()) {
                        Ok(entry) => {
                            println!("{:<20} {:<20} {:<10} {:<15} {:<10} {:<30}", entry.module, entry.module_name, entry.version, entry.environment, entry.reference, entry.description);
                        },
                        Err(e) => {
                            // Handle parsing error
                            error!("Failed to parse `manifest` into `ModuleManifest`: {}", e);
                        }
                    }
                }
            }
        } else {
            error!("Response JSON does not contain a double-encoded array as expected.");
        }
    } else {
        println!("No payload in response");
    }
    Ok([].to_vec())
}


pub async fn list_environments() -> Result<Vec<EnvironmentResp>, anyhow::Error> {
    let function_name = "api_module";
    let payload = serde_json::json!({
        "event": "list_environments"
    });

    if let Ok(response_json) = run_function(function_name, payload).await {

        // Check if response_json is a string that needs to be parsed as JSON.
        if let serde_json::Value::String(encoded_array) = &response_json {
            // The string is double-encoded JSON; parse it to get the array.
            let environments_array: serde_json::Value = serde_json::from_str(encoded_array)
                .expect("Failed to parse double-encoded JSON");

            if let serde_json::Value::Array(environments) = environments_array {
                let mut environments_resp: Vec<EnvironmentResp> = Vec::new();

                let datetime = Local.timestamp(0, 0);
                let utc_offset = datetime.format("%:z").to_string();
                let simplified_offset = simplify_utc_offset(&utc_offset);

                println!("{:<25} {:<15}", "Environments", format!("LastActivity ({})", simplified_offset));
                for environment in &environments {
                    // println!("{:?}", module);
                    match serde_json::from_value::<EnvironmentResp>(environment.clone()) {
                        Ok(entry) => {
                            let datetime = Local.timestamp(entry.last_activity_epoch, 0);
                            let date_string = datetime.format("%Y-%m-%d %H:%M:%S").to_string();
                            println!("{:<25} {:<15}", entry.environment, date_string);
                            environments_resp.push(entry);
                        },
                        Err(e) => {
                            // Handle parsing error
                            error!("Failed to parse `manifest` into `EnvironmentManifest`: {}", e);
                        }
                    }
                }
            }
        } else {
            error!("Response JSON does not contain a double-encoded array as expected.");
        }
    } else {
        println!("No payload in response");
    }

    Ok([].to_vec())
}

pub async fn get_module_version(module: &String, version: &String) -> anyhow::Result<ModuleResp> {
    let function_name = "api_module";
    let payload = serde_json::json!({
        "event": "get_module",
        "module": module,
        "version": version
    });
        
    if let Ok(response_json) = run_function(function_name, payload).await {

        // Check if response_json is a string that needs to be parsed as JSON.
        if let serde_json::Value::String(encoded_array) = &response_json {
            // The string is double-encoded JSON; parse it to get the array.
            if let Ok(module) = serde_json::from_str::<ModuleResp>(encoded_array) {
                let yaml_string = serde_yaml::to_string(&module.manifest).unwrap();
                println!("Information\n------------");
                println!("Module: {}", module.module);
                println!("Version: {}", module.version);
                println!("Description: {}", module.description);
                println!("Reference: {}", module.reference);
                println!("\n");

                println!("{}", yaml_string);
                return Ok(module)
            } else {
                println!("Could not parse inner JSON string");
                return Err(anyhow::anyhow!("Could not parse inner JSON string"));
            }
        }else{
            println!("Could not parse inner JSON string");
            return Err(anyhow::anyhow!("Could not parse inner JSON string"));
        }
    } else {
        println!("No payload in response");
        return Err(anyhow::anyhow!("No payload in response"));
    }
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