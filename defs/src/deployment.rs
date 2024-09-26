use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct DeploymentResp {
    pub epoch: u128,
    pub deployment_id: String,
    pub status: String,
    pub environment: String,
    pub module: String,
    pub module_version: String,
    pub variables: serde_json::Value,
    pub error_text: String,
    pub deleted: bool,
}
