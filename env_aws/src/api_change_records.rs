use env_defs::InfraChangeRecord;
use env_utils::merge_json_dicts;
use log::error;
use serde_json::Value;

use crate::api::run_lambda;

pub async fn insert_infra_change_record(
    infra_change_record: InfraChangeRecord,
    plan_output_raw: &str,
) -> Result<String, anyhow::Error> {

    match upload_plan_output_file(&infra_change_record.plan_raw_json_key, plan_output_raw).await {
        Ok(_) => {
            println!("Successfully uploaded plan output file");
        },
        Err(e) => {
            error!("Failed to upload plan output file: {}", e);
            println!("Failed to upload plan output file: {}", e);
        }
    }

    let pk_prefix = match infra_change_record.change_type.as_str() {
        "apply" => "APPLY",
        "plan" => "PLAN",
        _ => "UNKNOWN",
    };

    let pk = format!(
        "{}#{}",
        pk_prefix,
        get_identifier(
            &infra_change_record.deployment_id,
            &infra_change_record.environment
        )
    );

    let mut infra_change_record_payload = serde_json::to_value(serde_json::json!({
        "PK": &pk,
        "SK": &infra_change_record.job_id,
    }))
    .unwrap();
    let infra_change_record_value = serde_json::to_value(&infra_change_record).unwrap();
    merge_json_dicts(&mut infra_change_record_payload, &infra_change_record_value);


    println!(
        "Invoking Lambda with payload: {}",
        infra_change_record_value
    );

    let payload = serde_json::json!({
        "event": "insert_db",
        "table": "change_records",
        "data": &infra_change_record_payload
    });

    match run_lambda(payload).await {
        Ok(_) => Ok("".to_string()),
        Err(e) => {
            error!("Failed to insert event: {}", e);
            println!("Failed to insert event: {}", e);
            Err(anyhow::anyhow!("Failed to insert event: {}", e))
        }
    }
}

async fn upload_plan_output_file(key: &str, content: &str) -> Result<String, anyhow::Error> {
    let base64_content = base64::encode(content);

    let payload = serde_json::json!({
        "event": "upload_file_base64",
        "data":
        {
            "key": key,
            "bucket_name": "change_records",
            "base64_content": base64_content
        }
    });

    match run_lambda(payload).await {
        Ok(_) => Ok("".to_string()),
        Err(e) => {
            error!("Failed to upload file: {}", e);
            println!("Failed to upload file: {}", e);
            Err(anyhow::anyhow!("Failed to upload file: {}", e))
        }
    }
}

pub async fn get_change_record(deployment_id: &str, environment: &str, job_id: &str) -> Result<InfraChangeRecord, anyhow::Error> {
    let response = read_db(serde_json::json!({
        "KeyConditionExpression": "PK = :pk AND SK = :sk",
        "ExpressionAttributeValues": {
            ":pk": format!("PLAN#{}", get_identifier(deployment_id, environment)),
            ":sk": job_id
        }
    }))
    .await?;

    let items = response.get("Items").expect("Items not found");

    if let Some(change_records) = items.as_array() {
        if change_records.len() == 1 {
            let change_record: InfraChangeRecord =
                serde_json::from_value(change_records[0].clone()).expect("Failed to parse change record");
            return Ok(change_record);
        } else {
            panic!("Expected exactly one change record");
        }
    } else {
        panic!("Expected an array of change records");
    }
}

async fn read_db(query: Value) -> Result<Value, anyhow::Error> { // TODO move this to a common module and reuse
    let payload = serde_json::json!({
        "event": "read_db",
        "table": "change_records",
        "data": {
            "query": query
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

fn get_identifier(deployment_id: &str, environment: &str) -> String {
    format!("{}::{}", environment, deployment_id)
}
