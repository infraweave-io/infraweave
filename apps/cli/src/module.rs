use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct ModuleResp {
    pub environment: String,
    pub version: String,
    pub timestamp: String,
    #[serde(rename = "module_name")]
    pub module_name: String,
    pub module: String,
    pub description: String,
    pub reference: String,
    pub manifest: ModuleManifest,  // Adjusted to use ModuleManifest
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct ModuleManifest {
    pub metadata: Metadata,
    #[serde(rename = "apiVersion")]
    pub api_version: String,
    pub kind: String,
    pub spec: ModuleSpec,  // Now properly includes ModuleSpec
}

// This struct represents the actual spec part of the manifest
#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct ModuleSpec {
    #[serde(rename = "moduleName")]
    pub module_name: String,
    pub version: String,
    pub parameters: Vec<Parameter>,
    pub outputs: Vec<Output>,  // Added to capture the outputs array
    pub provider: String,  // Added
    pub source: Source,  // Added, assuming a generic Source that can be S3 or Git
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Metadata {
    name: String,
    group: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Parameter {
    name: String,
    #[serde(rename = "type")]
    type_: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Output {  // This struct is added to match the outputs array
    name: String,
    #[serde(rename = "type")]
    type_: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "type")] // Use the 'type' field for distinguishing between variants
pub enum Source {
    #[serde(rename = "S3")]
    S3(S3Spec),
    #[serde(rename = "Git")]
    Git(GitSpec),
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct S3Spec {
    bucket: String,
    path: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct GitSpec {
    url: String,
    #[serde(rename = "ref")]
    ref_: String,
}