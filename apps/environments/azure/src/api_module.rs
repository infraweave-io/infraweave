use aws_sdk_lambda::{Client, Error}; // TO REMOVE
use aws_config::meta::region::RegionProviderChain; // TO REMOVE
use aws_sdk_lambda::primitives::Blob; // TO REMOVE
use serde_json::json;
use anyhow::Result;

use chrono::{TimeZone, Utc, Local};

use crate::module::ModuleResp;
use crate::environment::EnvironmentResp;

pub async fn publish_module(manifest_path: &String, environment: &String, description: &String, reference: &String) -> Result<()> {
    let client = reqwest::Client::new();

    let manifest = std::fs::read_to_string(manifest_path)
        .expect("Failed to read module manifest file");

    let function_url = "https://example-function-appmar.azurewebsites.net/api/api_module";
    let function_key = "***REMOVED***";

    let payload = json!({
        "event": "insert",
        "manifest": manifest,
        "environment": environment,
        "description": description,
        "reference": reference,
    });

    let response = client
        .post(function_url)
        .header("x-functions-key", function_key)
        .json(&payload)
        .send()
        .await?
        .text()
        .await?;

    println!("Response payload: {}", response);

    Ok(())
}



pub async fn list_latest(environment: &String) -> Result<(Vec<ModuleResp>), Error> {
    let region_provider = RegionProviderChain::default_provider().or_else("us-east-1");
    let shared_config = aws_config::from_env().region(region_provider).load().await;
    let client = Client::new(&shared_config);

    let function_name = "moduleApi";
    let payload = serde_json::json!({
        "event": "list_latest",
        "environment": environment
    });

    let response = client.invoke()
        .function_name(function_name)
        .payload(Blob::new(serde_json::to_vec(&payload).unwrap()))
        .send()
        .await?;
        
    if let Some(blob) = response.payload {
        let payload_bytes = blob.into_inner();
        let payload_str = String::from_utf8(payload_bytes).expect("Failed to convert payload to String");
    
        // Attempt to parse the payload string to serde_json::Value
        let parsed: serde_json::Value = serde_json::from_str(&payload_str).expect("Failed to parse string to JSON Value");
        
        // Conditionally check and further parse if the value is a string
        if let Some(inner_json_str) = parsed.as_str() {
            // If the parsed value is a string, it might be another layer of JSON string
            let modules: Vec<ModuleResp> = serde_json::from_str(inner_json_str).expect("Failed to parse inner JSON string");
            
            println!("{:<10} {:<15} {:<10} {:<15} {:<10} {:<30}", "Module", "ModuleName", "Version", "Environment", "Ref", "Description");
            for entry in &modules {
                println!("{:<10} {:<15} {:<10} {:<15} {:<10} {:<30}", entry.module, entry.module_name, entry.version, entry.environment, entry.reference, entry.description);
            }
            return Ok(modules)
        }else{
            println!("No payload in response");
        }
    } else {
        println!("No payload in response");
    }
    Ok([].to_vec())
}


pub async fn list_environments() -> Result<(), Error> {
    let region_provider = RegionProviderChain::default_provider().or_else("us-east-1");
    let shared_config = aws_config::from_env().region(region_provider).load().await;
    let client = Client::new(&shared_config);

    let function_name = "moduleApi";
    let payload = serde_json::json!({
        "event": "list_environments"
    });

    let response = client.invoke()
        .function_name(function_name)
        .payload(Blob::new(serde_json::to_vec(&payload).unwrap()))
        .send()
        .await?;

    if let Some(blob) = response.payload {
        let payload_bytes = blob.into_inner();
        let payload_str = String::from_utf8(payload_bytes).expect("Failed to convert payload to String");
    
        // Attempt to parse the payload string to serde_json::Value
        let parsed: serde_json::Value = serde_json::from_str(&payload_str).expect("Failed to parse string to JSON Value");
        
        // Conditionally check and further parse if the value is a string
        if let Some(inner_json_str) = parsed.as_str() {
            // If the parsed value is a string, it might be another layer of JSON string
            let modules: Vec<EnvironmentResp> = serde_json::from_str(inner_json_str).expect("Failed to parse inner JSON string");
            
            let datetime = Local.timestamp(0, 0);
            let utc_offset = datetime.format("%:z").to_string();
            let simplified_offset = simplify_utc_offset(&utc_offset);

            println!("{:<25} {:<15}", "Environments", format!("LastActivity ({})", simplified_offset));
            for entry in modules {
                let datetime = Local.timestamp(entry.last_activity_epoch, 0);
                let date_string = datetime.format("%Y-%m-%d %H:%M:%S").to_string();
                println!("{:<25} {:<15}", entry.environment, date_string);
            }
        }

    } else {
        println!("No payload in response");
    }

    Ok(())
}

pub async fn get_module_version(module: &String, version: &String) ->  anyhow::Result<ModuleResp> {
    let region_provider = RegionProviderChain::default_provider().or_else("us-east-1");
    let shared_config = aws_config::from_env().region(region_provider).load().await;
    let client = Client::new(&shared_config);

    let function_name = "moduleApi";
    let payload = serde_json::json!({
        "event": "get_module",
        "module": module,
        "version": version
    });

    let response = client.invoke()
        .function_name(function_name)
        .payload(Blob::new(serde_json::to_vec(&payload).unwrap()))
        .send()
        .await?;
        
    if let Some(blob) = response.payload {
        let payload_bytes = blob.into_inner();
        let payload_str = String::from_utf8(payload_bytes).expect("Failed to convert payload to String");
    
        // Attempt to parse the payload string to serde_json::Value
        let parsed: serde_json::Value = serde_json::from_str(&payload_str).expect("Failed to parse string to JSON Value");
        
        // Conditionally check and further parse if the value is a string
        if let Some(inner_json_str) = parsed.as_str() {
            // If the parsed value is a string, it might be another layer of JSON string
            let module: ModuleResp = serde_json::from_str(inner_json_str).expect("Failed to parse inner JSON string");
            
            let yaml_string = serde_yaml::to_string(&module.manifest).unwrap();
            println!("Information\n------------");
            println!("Module: {}", module.module);
            println!("Version: {}", module.version);
            println!("Description: {}", module.description);
            println!("Reference: {}", module.reference);
            println!("\n");

            println!("{}", yaml_string);
            return Ok(module)
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