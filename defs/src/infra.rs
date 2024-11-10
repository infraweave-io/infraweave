use serde::{Deserialize, Serialize};

use crate::deployment::{Dependency, DriftDetection};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ApiInfraPayload {
    pub command: String,
    pub args: Vec<String>,
    pub module: String,
    pub module_version: String,
    pub module_type: String,
    pub module_track: String,
    pub name: String,
    pub environment: String,
    pub deployment_id: String,
    pub project_id: String,
    pub region: String,
    pub drift_detection: DriftDetection,
    pub next_drift_check_epoch: i128,
    pub variables: serde_json::value::Value,
    pub annotations: serde_json::value::Value,
    pub dependencies: Vec<Dependency>,
    pub initiated_by: String,
}
