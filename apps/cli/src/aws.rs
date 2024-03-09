use aws_sdk_lambda::{Client, Error};
use aws_config::meta::region::RegionProviderChain;
use aws_sdk_lambda::primitives::Blob;

use chrono::{TimeZone, Utc, Local};

use crate::module::ModuleResp;
use crate::environment::EnvironmentResp;

pub async fn publish_module(manifest_path: &String, environment: &String, description: &String, reference: &String) -> Result<(), Error> {
    let region_provider = RegionProviderChain::default_provider().or_else("us-east-1");
    let shared_config = aws_config::from_env().region(region_provider).load().await;
    let client = Client::new(&shared_config);

    let manifest = std::fs::read_to_string(manifest_path)
        .expect("Failed to read module manifest file");

    let function_name = "moduleApi";
    let payload = serde_json::json!({
        "event": "insert",
        "manifest": manifest,
        "environment": environment,
        "description": description,
        "reference": reference
    });

    let response = client.invoke()
        .function_name(function_name)
        .payload(Blob::new(serde_json::to_vec(&payload).unwrap()))
        .send()
        .await?;


    if let Some(log_result) = response.log_result {
        println!("Log result: {}", log_result);
    }

    if let Some(payload) = response.payload {
        let payload_bytes: Vec<u8> = payload.into_inner(); // Convert Blob to Vec<u8>
        let payload_str = String::from_utf8(payload_bytes)
            .expect("Failed to convert payload to String");
        println!("Response payload: {}", payload_str);
    }

    Ok(())
}



pub async fn list_latest(environment: &String) -> Result<(), Error> {
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
            for entry in modules {
                println!("{:<10} {:<15} {:<10} {:<15} {:<10} {:<30}", entry.module, entry.module_name, entry.version, entry.environment, entry.reference, entry.description);
            }
        }

    } else {
        println!("No payload in response");
    }

    Ok(())
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