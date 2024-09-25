use crate::api::run_lambda;
use env_defs::EventData;
use log::{error, warn};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Serialize, Deserialize)]
struct ApiEventLambdaPayload {
    deployment_id: String,
    query: Value,
}

pub async fn insert_event(event: EventData) -> anyhow::Result<String> {
    // create json with event data {"event": "db_insert", "data": event}
    let payload = serde_json::json!({
        "event": "insert_db",
        "table": "events",
        "data": event
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

pub async fn get_events(deployment_id: &String) -> anyhow::Result<Vec<EventData>, anyhow::Error> {
    let response = read_db(serde_json::json!({
        "KeyConditionExpression": "deployment_id = :deployment_id",
        "ExpressionAttributeValues": {":deployment_id": deployment_id}
    }))
    .await?;

    let items = response.get("Items").expect("Items not found");

    if let Some(events) = items.as_array() {
        let mut events_vec: Vec<EventData> = vec![];
        for event in events {
            warn!("Event: {:?}", event);
            let eventdata: EventData =
                serde_json::from_value(event.clone()).expect("Failed to parse event");
            events_vec.push(eventdata);
        }
        return Ok(events_vec);
    } else {
        panic!("Expected an array of events");
    }
}

async fn read_db(query: Value) -> Result<Value, anyhow::Error> {
    let payload = ApiEventLambdaPayload {
        deployment_id: "".to_string(),
        query: query,
    };

    let payload = serde_json::json!({
        "event": "read_db",
        "table": "events",
        "data": payload
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

    Ok(response)
}
