use serde::{Deserialize, Serialize};
use serde_json::Value;

pub fn get_deployment_identifier(
    project_id: &str,
    region: &str,
    deployment_id: &str,
    environment: &str,
) -> String {
    if environment.is_empty() {
        format!("{}::{}", project_id, region)
    } else if environment.is_empty() && deployment_id.is_empty() {
        format!("{}::{}::{}", project_id, region, environment)
    } else {
        format!(
            "{}::{}::{}::{}",
            project_id, region, environment, deployment_id
        )
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Metadata {
    pub name: String,
    pub namespace: Option<String>,
    pub annotations: Option<serde_yaml::Mapping>,
    pub labels: Option<serde_yaml::Mapping>,
    // pub group: String,
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct DeploymentManifest {
    pub metadata: Metadata,
    #[serde(rename = "apiVersion")]
    pub api_version: String,
    pub kind: String,
    pub spec: DeploymentSpec,
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct DeploymentSpec {
    // pub name: String,
    #[serde(rename = "moduleVersion")]
    pub module_version: Option<String>,
    #[serde(rename = "stackVersion")]
    pub stack_version: Option<String>,
    pub region: String,
    // pub description: String,
    pub reference: Option<String>,
    pub variables: serde_yaml::Mapping,
    pub dependencies: Option<Vec<DependencySpec>>,
    #[serde(rename = "driftDetection")]
    pub drift_detection: Option<DriftDetection>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DependencySpec {
    #[serde(rename = "deploymentId")]
    pub deployment_id: String,
    pub environment: String,
}

// Manifest above (camelCase), Database data below (snake_case)
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Deserialize, Clone, Debug, Serialize)]
pub struct DeploymentResp {
    pub epoch: u128,
    pub deployment_id: String,
    pub status: String,
    pub job_id: String,
    pub environment: String,
    pub project_id: String,
    pub region: String,
    pub module: String,
    pub module_version: String,
    pub module_type: String,
    pub module_track: String,
    pub drift_detection: DriftDetection,
    pub next_drift_check_epoch: i128,
    pub has_drifted: bool,
    pub variables: Value,
    pub output: Value,
    pub policy_results: Vec<crate::PolicyResult>,
    pub error_text: String,
    pub deleted: bool,
    pub dependencies: Vec<Dependency>,
    pub initiated_by: String,
    pub cpu: String,
    pub memory: String,
    pub reference: String,
}

#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Dependency {
    pub project_id: String,
    pub region: String,
    pub deployment_id: String,
    pub environment: String,
}

#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Dependent {
    pub project_id: String,
    pub region: String,
    pub dependent_id: String,
    pub environment: String,
}

#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RepositoryData {
    pub git_provider: String,
    pub git_url: String,
    pub repository_path: String,
    #[serde(rename = "type")]
    pub _type: String,
}

#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ProjectData {
    pub project_id: String,
    pub name: String,
    pub description: String,
    pub regions: Vec<String>,
    pub repositories: Vec<RepositoryData>,
}

#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DriftDetection {
    #[serde(default = "default_drift_detection_false")]
    pub enabled: bool,

    #[serde(default = "default_drift_detection_interval")]
    pub interval: String,

    #[serde(default = "default_drift_detection_false")]
    pub auto_remediate: bool,

    #[serde(default = "default_drift_detection_empty_list")]
    pub webhooks: Vec<Webhook>,
}

#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Webhook {
    pub url: Option<String>,
    // TODO: Add alias to provide option to avoid having to provide sensitive url in the config
}

fn default_drift_detection_false() -> bool {
    false
}

fn default_drift_detection_empty_list() -> Vec<Webhook> {
    vec![]
}

pub const DEFAULT_DRIFT_DETECTION_INTERVAL: &str = "15m"; // Also used in CLI, hence

fn default_drift_detection_interval() -> String {
    DEFAULT_DRIFT_DETECTION_INTERVAL.to_string()
}
