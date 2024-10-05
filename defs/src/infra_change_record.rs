use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct InfraChangeRecord {
    pub deployment_id: String,
    pub job_id: String,
    pub module: String,
    pub environment: String,
    pub change_type: String, // plan or apply
    pub module_version: String,
    pub epoch: u128,
    pub timestamp: String,
    pub plan_std_output: String,
    pub plan_raw_json_key: String,
}
