use serde::{Deserialize, Serialize};

use crate::deployment::Dependency;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ApiInfraPayload {
    pub command: String,
    pub module: String,
    pub module_version: String,
    pub name: String,
    pub environment: String,
    pub deployment_id: String,
    pub variables: serde_json::value::Value,
    pub annotations: serde_json::value::Value,
    pub dependencies: Vec<Dependency>,
}
