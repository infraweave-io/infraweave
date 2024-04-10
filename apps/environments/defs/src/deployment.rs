use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct DeploymentResp {
    pub cloud_id: String,
    pub cloud_type: String,
    pub deployment_id: String,
    // pub name: String,
    // pub environment: String,
    // pub module: String,
    // pub last_activity_epoch: i64,
}
