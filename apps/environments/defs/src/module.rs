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
    #[serde(deserialize_with = "deserialize_manifest")]
    pub manifest: ModuleManifest,
}


fn deserialize_manifest<'de, D>(deserializer: D) -> Result<ModuleManifest, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let s: String = Deserialize::deserialize(deserializer)?;
    serde_json::from_str(&s).map_err(serde::de::Error::custom)
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
    pub name: String,
    // pub group: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Parameter {
    pub name: String,
    #[serde(rename = "type")]
    pub type_: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Output {  // This struct is added to match the outputs array
    pub name: String,
    #[serde(rename = "type")]
    pub type_: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "type")] // Use the 'type' field for distinguishing between variants
pub enum Source {
    #[serde(rename = "S3")]
    S3(S3Spec),
    #[serde(rename = "Git")]
    Git(GitSpec),
    #[serde(rename = "StorageContainer")]
    StorageContainer(StorageContainerSpec),
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

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct StorageContainerSpec {
    #[serde(rename = "storageAccount")]
    storage_account: String,
    path: String,
}