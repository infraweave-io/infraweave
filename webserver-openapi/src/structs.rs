use env_defs::{deserialize_manifest, ModuleManifest, TfOutput, TfVariable};
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
pub struct ModuleV1 {
    pub environment: String,
    pub environment_version: String,
    pub version: String,
    pub timestamp: String,
    #[serde(rename = "module_name")]
    pub module_name: String,
    pub module: String,
    pub description: String,
    pub reference: String,
    #[serde(deserialize_with = "deserialize_manifest")]
    pub manifest: ModuleManifest,
    pub tf_variables: Vec<TfVariable>,
    pub tf_outputs: Vec<TfOutput>,
    pub s3_key: String,
}

#[derive(Deserialize, Serialize, Clone, Debug, utoipa::ToSchema)]
pub struct DeploymentV1 {
    pub epoch: u128,
    pub deployment_id: String,
    pub status: String,
    pub environment: String,
    pub module: String,
    pub module_version: String,
    pub variables: serde_json::Value,
    pub error_text: String,
    pub deleted: bool,
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
