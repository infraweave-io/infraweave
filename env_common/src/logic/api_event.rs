use env_defs::{get_event_identifier, EventData};
use env_utils::{get_epoch, merge_json_dicts};

use crate::interface::CloudHandler;

use super::common::handler;

pub async fn insert_event(event: EventData) -> Result<String, anyhow::Error> {
    let id: String = format!(
        "EVENT#{}",
        get_event_identifier(
            &event.project_id,
            &event.region,
            &event.deployment_id,
            &event.environment
        )
    );

    let pk_base_region = format!("EVENT#{}", &event.region);

    let mut event_payload = serde_json::to_value(serde_json::json!({
        "PK": id.clone(),
        "SK": get_epoch().to_string(),
        "PK_base_region": pk_base_region,
    }))
    .unwrap();

    let event_value = serde_json::to_value(&event).unwrap();
    merge_json_dicts(&mut event_payload, &event_value);

    let payload = serde_json::json!({
        "event": "insert_db",
        "table": "events",
        "data": &event_payload
    });

    match handler().run_function(&payload).await {
        Ok(_) => Ok("".to_string()),
        Err(e) => Err(anyhow::anyhow!("Failed to insert event: {}", e)),
    }
}
