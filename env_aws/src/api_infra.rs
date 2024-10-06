use env_defs::ApiInfraPayload;
use log::error;
use serde_json::Value;

use crate::api::run_lambda;

pub async fn mutate_infra(payload: ApiInfraPayload) -> anyhow::Result<Value> {
    let payload = serde_json::json!({
        "event": "start_runner",
        "data": payload
    });

    match run_lambda(payload).await {
        Ok(resp) => Ok(resp),
        Err(e) => {
            error!("Failed to insert event: {}", e);
            println!("Failed to insert event: {}", e);
            Err(anyhow::anyhow!("Failed to insert event: {}", e))
        }
    }
}
