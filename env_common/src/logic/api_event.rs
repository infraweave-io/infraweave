use env_defs::EventData;

use crate::interface::CloudHandler;

use super::common::handler;

pub async fn insert_event(event: EventData) -> Result<String, anyhow::Error> {
    let payload = serde_json::json!({
        "event": "insert_db",
        "table": "events",
        "data": event
    });

    match handler().run_function(&payload).await {
        Ok(_) => Ok("".to_string()),
        Err(e) => {
            Err(anyhow::anyhow!("Failed to insert event: {}", e))
        }
    }
}

pub async fn get_events(deployment_id: &String) -> Result<Vec<EventData>, anyhow::Error> {
    handler().get_events(deployment_id).await
}