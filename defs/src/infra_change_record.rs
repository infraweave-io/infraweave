use serde::{Deserialize, Serialize};

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
    // TODO: add variables since it might be interesting here
    pub epoch: u128,
    pub timestamp: String,
    /// Human-readable text output from terraform command.
    /// For plan commands: contains `terraform plan` stdout showing planned changes.
    /// For apply/destroy commands: contains `terraform apply/destroy` stdout showing
    /// what actually happened (including "Apply complete! Resources: 2 added, 0 changed, 0 destroyed").
    pub plan_std_output: String,
    /// Storage key/path to the raw JSON output from terraform show.
    /// For plan commands: points to `{command}_{job_id}_plan_output.json`
    /// For apply commands: points to `{command}_{job_id}_apply_output.json`
    /// The actual file path distinguishes between planned vs applied changes.
    pub plan_raw_json_key: String,
}
