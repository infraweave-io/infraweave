use std::collections::HashMap;

use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct DeploymentResp {
    pub epoch: i64,
    pub deployment_id: String,
    pub environment: String,
    pub module: String,
    pub inputs: HashMap<String, String>,
}
