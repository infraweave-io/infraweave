use env_defs::InfraChangeRecord;
use env_utils::merge_json_dicts;

use crate::interface::CloudHandler;

use super::common::handler;

fn get_identifier(deployment_id: &str, environment: &str) -> String {
    format!("{}::{}", environment, deployment_id)
}

pub async fn get_change_record(environment: &str, deployment_id: &str, job_id: &str, change_type: &str) -> Result<InfraChangeRecord, anyhow::Error> {
    handler().get_change_record(environment, deployment_id, job_id, change_type).await
}

pub async fn insert_infra_change_record(
    infra_change_record: InfraChangeRecord,
    plan_output_raw: &str,
) -> Result<String, anyhow::Error> {

    match upload_plan_output_file(&infra_change_record.plan_raw_json_key, plan_output_raw).await {
        Ok(_) => {
            println!("Successfully uploaded plan output file");
        },
        Err(e) => {
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

    match handler().run_function(&payload).await {
        Ok(_) => Ok("".to_string()),
        Err(e) => {
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

    match handler().run_function(&payload).await {
        Ok(_) => Ok("".to_string()),
        Err(e) => {
            Err(anyhow::anyhow!("Failed to upload file: {}", e))
        }
    }
}