use serde::{Deserialize, Serialize};

use crate::ModuleExample;

// These are only used to parse files, they will be stored as modules in DB

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Metadata {
    pub name: String,
    // pub group: String,
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct StackManifest {
    pub metadata: Metadata,
    #[serde(rename = "apiVersion")]
    pub api_version: String,
    pub kind: String,
    pub spec: StackSpec,
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct StackSpec {
    #[serde(rename = "stackName")]
    pub stack_name: String,
    pub version: Option<String>,
    pub description: String,
    pub reference: String,
    pub examples: Option<Vec<ModuleExample>>
}
