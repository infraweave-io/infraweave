use env_defs::{deserialize_module_manifest, deserialize_policy_manifest, DriftDetection, ModuleManifest, ModuleStackData, ModuleVersionDiff, PolicyManifest, PolicyResult, TfOutput, TfVariable};
use serde::{Deserialize, Serialize};

// Redefine the structs here so that it can be used in the imported module
// conform to versioned api in case something changes. This is intended to be
// a safe guard to ensure that users of this api can rely on it.
#[derive(Debug, Serialize, Deserialize, utoipa::ToSchema)]
pub struct ApiInfraPayload {
    pub command: String,
    pub module: String,
    pub module_version: String,
    pub name: String,
    pub environment: String,
    pub deployment_id: String,
    pub variables: serde_json::value::Value,
    pub annotations: serde_json::value::Value,
}

#[derive(Deserialize, Serialize, Clone, Debug, utoipa::ToSchema)]
pub struct EventData {
    pub deployment_id: String,
    pub event: String,
    pub epoch: u128,
    pub error_text: String,
    pub id: String,
    pub job_id: String,
    pub metadata: serde_json::Value,
    pub module: String,
    pub name: String,
    pub status: String,
    pub timestamp: String,
}

#[derive(Deserialize, Serialize, Clone, Debug, utoipa::ToSchema)]
pub struct PolicyV1 {
    pub environment: String,
    pub environment_version: String,
    pub version: String,
    pub timestamp: String,
    pub policy_name: String,
    pub policy: String,
    pub description: String,
    pub reference: String,
    pub data: serde_json::Value,
    #[serde(deserialize_with = "deserialize_policy_manifest")]
    pub manifest: PolicyManifest,
    pub s3_key: String,
}

#[derive(Deserialize, Serialize, Clone, Debug, utoipa::ToSchema)]
pub struct ModuleV1 {
    pub track: String,
    pub track_version: String,
    pub version: String,
    pub timestamp: String,
    #[serde(rename = "module_name")]
    pub module_name: String,
    pub module: String,
    pub description: String,
    pub reference: String,
    #[serde(deserialize_with = "deserialize_module_manifest")]
    pub manifest: ModuleManifest,
    pub tf_variables: Vec<TfVariable>,
    pub tf_outputs: Vec<TfOutput>,
    pub s3_key: String,
    pub stack_data: Option<ModuleStackData>,
    pub version_diff: Option<ModuleVersionDiff>,
}

#[derive(Deserialize, Serialize, Clone, Debug, utoipa::ToSchema)]
pub struct DeploymentV1 {
    pub epoch: u128,
    pub deployment_id: String,
    pub status: String,
    pub job_id: String,
    pub environment: String,
    pub module: String,
    pub module_version: String,
    pub module_type: String,
    pub module_track: String,
    pub drift_detection: DriftDetection,
    pub next_drift_check_epoch: i128,
    pub has_drifted: bool,
    pub variables: serde_json::Value,
    pub output: serde_json::Value,
    pub policy_results: Vec<PolicyResult>,
    pub error_text: String,
    pub deleted: bool,
    pub dependencies: Vec<DependencyV1>, // TODO REMOVE THIS Use DependencyV1 instead of Dependency since it has a different serializer
    pub dependants: Vec<DependantsV1>, // Use DependantsV1 instead of Dependent since it is fetched differently
    pub initiated_by: String,
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct DependencyV1 {
    pub deployment_id: String,
    pub environment: String,
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct DependantsV1 {
    pub deployment_id: String,
    pub environment: String,
}

#[derive(Deserialize, Serialize, Clone, Debug, utoipa::ToSchema)]
pub struct EventV1 {
    pub deployment_id: String,
    pub event: String,
    pub epoch: u128,
    pub error_text: String,
    pub id: String,
    pub job_id: String,
    pub metadata: serde_json::Value,
    pub module: String,
    pub name: String,
    pub status: String,
    pub timestamp: String,
}

#[derive(Deserialize, Serialize, Clone, Debug, utoipa::ToSchema)]
pub struct ProjectDataV1 {
    pub project_id: String,
    pub name: String,
    pub description: String,
    pub regions: Vec<String>,
}
