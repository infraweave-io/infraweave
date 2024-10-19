use env_defs::{ApiInfraPayload, GenericFunctionResponse};

use crate::interface::CloudHandler;

use super::common::handler;

pub async fn mutate_infra(payload: ApiInfraPayload) -> Result<GenericFunctionResponse, anyhow::Error> {
    let payload = serde_json::json!({
        "event": "start_runner",
        "data": payload
    });

    match handler().run_function(&payload).await {
        Ok(resp) => Ok(resp),
        Err(e) => {
            Err(anyhow::anyhow!("Failed to insert event: {}", e))
        }
    }
}
