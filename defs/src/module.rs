use serde::{Deserialize, Serialize};

#[allow(dead_code)]
pub fn get_module_identifier(module: &str, track: &str) -> String {
    format!("{}::{}", track, module)
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TfVariable {
    pub name: String,
    #[serde(rename = "type")]
    pub _type: serde_json::Value,
    pub default: Option<serde_json::Value>,
    pub description: Option<String>,
    pub nullable: Option<bool>,
    pub sensitive: Option<bool>,
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct TfValidation {
    pub expression: String,
    pub message: String,
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct ModuleStackData {
    pub modules: Vec<StackModule>,
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct StackModule {
    pub module: String,
    pub version: String,
    pub s3_key: String,
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct TfOutput {
    pub name: String,
    pub value: String,
    pub description: String,
}

#[derive(Deserialize, Serialize, Clone, Debug, PartialEq)]
pub struct ModuleDiffAddition {
    pub path: String,
    pub value: serde_json::Value,
}

#[derive(Deserialize, Serialize, Clone, Debug, PartialEq)]
pub struct ModuleDiffRemoval {
    pub path: String,
    pub value: serde_json::Value,
}

#[derive(Deserialize, Serialize, Clone, Debug, PartialEq)]
pub struct ModuleDiffChange {
    pub path: String,
    pub old_value: serde_json::Value,
    pub new_value: serde_json::Value,
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct ModuleVersionDiff {
    pub added: Vec<ModuleDiffAddition>,
    pub changed: Vec<ModuleDiffChange>,
    pub removed: Vec<ModuleDiffRemoval>,
    pub previous_version: String,
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct ModuleResp {
    pub track: String,
    pub track_version: String,
    pub version: String,
    pub timestamp: String,
    #[serde(rename = "module_name")]
    pub module_name: String,
    pub module: String,
    pub module_type: String,
    pub description: String,
    pub reference: String,
    #[serde(deserialize_with = "deserialize_module_manifest")]
    pub manifest: ModuleManifest,
    pub tf_variables: Vec<TfVariable>,
    pub tf_outputs: Vec<TfOutput>, // Added to capture the outputs array
    pub s3_key: String,
    pub stack_data: Option<ModuleStackData>,
    pub version_diff: Option<ModuleVersionDiff>,
}

pub fn deserialize_module_manifest<'de, D>(deserializer: D) -> Result<ModuleManifest, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let val = Deserialize::deserialize(deserializer)?;
    let env = "aws"; // TODO: std::env::var("ENV").unwrap_or("aws".to_string());

    // Since Storage Database does not support map types, we need to deserialize the manifest as a string and then parse it
    // However AWS does support map types, so we can directly deserialize the manifest as a map
    match env {
        "aws" => {
            if let serde_json::Value::Object(map) = val {
                serde_json::from_value(serde_json::Value::Object(map))
                    .map_err(serde::de::Error::custom)
            } else {
                Err(serde::de::Error::custom(
                    "Expected a JSON object for AWS manifest",
                ))
            }
        }
        "azure" => {
            if let serde_json::Value::String(str) = val {
                serde_json::from_str(&str).map_err(serde::de::Error::custom)
            } else {
                Err(serde::de::Error::custom(
                    "Expected a JSON string for Azure manifest",
                ))
            }
        }
        _ => Err(serde::de::Error::custom("Invalid ENV value")),
    }
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct ModuleManifest {
    pub metadata: Metadata,
    #[serde(rename = "apiVersion")]
    pub api_version: String,
    pub kind: String,
    pub spec: ModuleSpec,
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct ModuleExample {
    pub name: String,
    pub description: String,
    pub variables: serde_yaml::Mapping,
}

// This struct represents the actual spec part of the manifest
#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct ModuleSpec {
    #[serde(rename = "moduleName")]
    pub module_name: String,
    pub version: Option<String>,
    pub description: String,
    pub reference: String,
    pub examples: Option<Vec<ModuleExample>>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Metadata {
    pub name: String,
    // pub group: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Output {
    // This struct is added to match the outputs array
    pub name: String,
    // #[serde(rename = "type")]
    // pub type_: String,
}

// TODO: remove below
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
