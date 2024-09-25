use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct EventData {
    pub deployment_id: String,
    pub event: String,
    pub epoch: u128,
    pub error_text: String,
    pub id: String,
    pub job_id: String,
    pub metadata: serde_json::Value,
    pub module: String,
    pub name: String,
    pub status: String,
    pub timestamp: String,
}
