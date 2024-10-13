use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Metadata {
    pub name: String,
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
    pub module_version: String,
    // pub description: String,
    // pub reference: String,
    pub variables: serde_yaml::Mapping,
    pub dependencies: Option<Vec<Dependency>>,
}

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
    #[serde(rename = "deploymentId")]
    pub deployment_id: String,
    pub environment: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Dependent {
    pub dependent_id: String,
    pub environment: String,
}
