use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct EnvironmentResp {
    pub environment: String,
    pub last_activity_epoch: i64,
}
