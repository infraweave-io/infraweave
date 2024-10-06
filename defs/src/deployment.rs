use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Deserialize, Clone, Debug, Serialize)]
pub struct DeploymentResp {
    pub epoch: u128,
    pub deployment_id: String,
    pub status: String,
    pub job_id: String,
    pub environment: String,
    pub module: String,
    pub module_version: String,
    pub variables: Value,
    pub output: Value,
    pub policy_results: Vec<crate::PolicyResult>,
    pub error_text: String,
    pub deleted: bool,
    pub dependencies: Vec<Dependency>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Dependency {
    pub deployment_id: String,
    pub environment: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Dependent {
    pub dependent_id: String,
    pub environment: String,
}
