use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::resource_change::SanitizedResourceChange;

pub fn get_change_record_identifier(
    project_id: &str,
    region: &str,
    deployment_id: &str,
    environment: &str,
) -> String {
    format!(
        "{}::{}::{}::{}",
        project_id, region, environment, deployment_id
    )
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct InfraChangeRecord {
    pub deployment_id: String,
    pub project_id: String,
    pub region: String,
    pub job_id: String,
    pub module: String,
    pub environment: String,
    pub change_type: String, // plan or apply
    pub module_version: String,
    pub epoch: u128,
    pub timestamp: String,
    /// Human-readable terraform output (plan/apply/destroy stdout)
    pub plan_std_output: String,
    /// Storage key for raw Terraform plan JSON (from `terraform show -json planfile`).
    /// Always contains the plan, even for apply/destroy. Stored in blob storage for compliance.
    /// Use `resource_changes` for non-sensitive audit trails.
    pub plan_raw_json_key: String,
    /// Sanitized resource changes (addresses and actions only, no sensitive values).
    /// Optional for backward compatibility.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub resource_changes: Vec<SanitizedResourceChange>,
    pub variables: Value,
}
