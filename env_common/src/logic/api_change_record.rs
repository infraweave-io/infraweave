use base64::engine::general_purpose::STANDARD as base64;
use base64::Engine;
use env_defs::{get_change_record_identifier, CloudProvider, InfraChangeRecord};
use env_utils::merge_json_dicts;

use crate::interface::GenericCloudHandler;

pub async fn insert_infra_change_record(
    handler: &GenericCloudHandler,
    infra_change_record: InfraChangeRecord,
    plan_output_raw: &str,
    plan_stdout: &str,
) -> Result<String, anyhow::Error> {
    // Upload the raw plan JSON
    match upload_plan_output_file(
        handler,
        &infra_change_record.plan_raw_json_key,
        plan_output_raw,
    )
    .await
    {
        Ok(_) => {
            println!("Successfully uploaded plan JSON file");
        }
        Err(e) => {
            println!("Failed to upload plan JSON file: {}", e);
        }
    }

    // Upload the human-readable stdout only if it was truncated (key is set)
    if !infra_change_record.plan_std_output_key.is_empty() {
        match upload_plan_output_file(
            handler,
            &infra_change_record.plan_std_output_key,
            plan_stdout,
        )
        .await
        {
            Ok(_) => {
                println!("Successfully uploaded plan stdout file to S3 (output was truncated)");
            }
            Err(e) => {
                println!("Failed to upload plan stdout file: {}", e);
            }
        }
    } else {
        println!("Plan stdout stored inline in DB (small enough)");
    }

    let pk_prefix = match infra_change_record.change_type.as_str() {
        "apply" => "APPLY",
        "plan" => "PLAN",
        "destroy" => "DESTROY",
        _ => "UNKNOWN",
    };

    let pk = format!(
        "{}#{}",
        pk_prefix,
        get_change_record_identifier(
            &infra_change_record.project_id,
            &infra_change_record.region,
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

    match handler.run_function(&payload).await {
        Ok(_) => Ok("".to_string()),
        Err(e) => Err(anyhow::anyhow!("Failed to insert event: {}", e)),
    }
}

async fn upload_plan_output_file<T: CloudProvider>(
    handler: &T,
    key: &str,
    content: &str,
) -> Result<String, anyhow::Error> {
    let base64_content = base64.encode(content);

    let payload = serde_json::json!({
        "event": "upload_file_base64",
        "data":
        {
            "key": key,
            "bucket_name": "change_records",
            "base64_content": base64_content
        }
    });

    match handler.run_function(&payload).await {
        Ok(_) => Ok("".to_string()),
        Err(e) => Err(anyhow::anyhow!("Failed to upload file: {}", e)),
    }
}
