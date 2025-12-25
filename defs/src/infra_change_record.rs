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
    /// Human-readable terraform output (plan/apply/destroy stdout).
    /// If output is >100KB, this contains last ~50KB and full output is in S3.
    /// If truncated, full output is in S3 at plan_std_output_key.
    #[serde(default)]
    pub plan_std_output: String,
    /// Storage key for full human-readable terraform stdout.
    /// Only set if plan_std_output was truncated due to size.
    /// If empty/missing, the full output is in plan_std_output.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub plan_std_output_key: String,
    /// Storage key for raw Terraform plan JSON (from `terraform show -json planfile`).
    /// Always contains the plan, even for apply/destroy. Stored in blob storage for compliance.
    /// Use `resource_changes` for non-sensitive audit trails.
    pub plan_raw_json_key: String,
    /// Sanitized resource changes (addresses and actions only, no sensitive values).
    /// Optional for backward compatibility.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub resource_changes: Vec<SanitizedResourceChange>,
    /// Variables used for the deployment.
    /// Optional for backward compatibility with older change records.
    #[serde(default)]
    pub variables: Value,
}
