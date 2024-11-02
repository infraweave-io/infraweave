use serde::{Deserialize, Serialize};

use crate::{DriftDetection, PolicyResult};

pub fn get_event_identifier(project_id: &str, region: &str, deployment_id: &str, environment: &str) -> String {
    format!("{}::{}::{}::{}", project_id, region, environment, deployment_id)
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct EventData {
    pub deployment_id: String,
    pub project_id: String,
    pub region: String,
    pub environment: String,
    pub event: String,
    pub epoch: u128,
    pub error_text: String,
    pub id: String,
    pub job_id: String,
    pub metadata: serde_json::Value,
    pub drift_detection: DriftDetection,
    pub next_drift_check_epoch: i128,
    pub has_drifted: bool,
    pub module: String,
    pub name: String,
    pub status: String,
    pub timestamp: String,
    pub output: serde_json::Value,
    pub policy_results: Vec<PolicyResult>,
}
