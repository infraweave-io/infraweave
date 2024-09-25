use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct ApiInfraPayload {
    pub command: String,
    pub module: String,
    pub module_version: String,
    pub name: String,
    pub environment: String,
    pub deployment_id: String,
    pub variables: serde_json::value::Value,
    pub annotations: serde_json::value::Value,
}
