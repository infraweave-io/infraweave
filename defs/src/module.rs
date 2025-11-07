use serde::{de::Deserializer, Deserialize, Serialize};

use crate::{oci::OciArtifactSet, ProviderResp};

#[allow(dead_code)]
pub fn get_module_identifier(module: &str, track: &str) -> String {
    format!("{}::{}", track, module)
}

#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct TfVariable {
    pub name: String,
    #[serde(rename = "type")]
    pub _type: serde_json::Value,
    #[serde(
        default,
        deserialize_with = "deserialize_default_value_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub default: Option<serde_json::Value>, // Default: missing -> None, explicitly set null in terraform variable -> Some(Value::Null)
    pub description: String,
    pub nullable: bool,
    pub sensitive: bool,
}

// Custom deserializer to treat an explicit JSON null as Some(Value::Null), but missing field as None
fn deserialize_default_value_option<'de, D>(
    deserializer: D,
) -> Result<Option<serde_json::Value>, D::Error>
where
    D: Deserializer<'de>,
{
    let v = serde_json::Value::deserialize(deserializer)?;
    Ok(Some(v))
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct TfValidation {
    pub expression: String,
    pub message: String,
}

#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct ModuleStackData {
    pub modules: Vec<StackModule>,
}

#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct StackModule {
    pub module: String,
    pub version: String,
    pub s3_key: String,
    #[serde(default)]
    pub track: String,
}

#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct TfOutput {
    pub name: String,
    pub value: String,
    pub description: String,
}

#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Deserialize, Serialize, Clone, Debug, PartialEq)]
pub struct TfRequiredProvider {
    pub name: String,
    pub version: String,
    pub source: String,
}

#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Deserialize, Serialize, Clone, Debug, PartialEq)]
pub struct TfLockProvider {
    pub source: String,
    pub version: String,
}

#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Deserialize, Serialize, Clone, Debug, PartialEq)]
pub struct ModuleDiffAddition {
    pub path: String,
    pub value: serde_json::Value,
}

#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Deserialize, Serialize, Clone, Debug, PartialEq)]
pub struct ModuleDiffRemoval {
    pub path: String,
    pub value: serde_json::Value,
}

#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Deserialize, Serialize, Clone, Debug, PartialEq)]
pub struct ModuleDiffChange {
    pub path: String,
    pub old_value: serde_json::Value,
    pub new_value: serde_json::Value,
}

#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct ModuleVersionDiff {
    pub added: Vec<ModuleDiffAddition>,
    pub changed: Vec<ModuleDiffChange>,
    pub removed: Vec<ModuleDiffRemoval>,
    pub previous_version: String,
}

#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
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
    pub tf_outputs: Vec<TfOutput>,
    #[serde(default)]
    pub tf_providers: Vec<ProviderResp>,
    #[serde(default)]
    pub tf_required_providers: Vec<TfRequiredProvider>,
    #[serde(default)]
    pub tf_lock_providers: Vec<TfLockProvider>,
    #[serde(default)]
    pub tf_extra_environment_variables: Vec<String>,
    pub s3_key: String,
    pub oci_artifact_set: Option<OciArtifactSet>,
    pub stack_data: Option<ModuleStackData>,
    pub version_diff: Option<ModuleVersionDiff>,
    pub cpu: String,
    pub memory: String,
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

#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct ModuleManifest {
    pub metadata: Metadata,
    #[serde(rename = "apiVersion")]
    pub api_version: String,
    pub kind: String,
    pub spec: ModuleSpec,
}

#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct ModuleExample {
    pub name: String,
    pub description: String,
    pub variables: serde_yaml::Value,
}

// This struct represents the actual spec part of the manifest
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct ModuleSpec {
    #[serde(rename = "moduleName")]
    pub module_name: String,
    pub version: Option<String>,
    pub description: String,
    pub reference: String,
    pub examples: Option<Vec<ModuleExample>>,
    pub cpu: Option<String>,
    pub memory: Option<String>,
    #[serde(default)]
    pub providers: Vec<Provider>,
}

#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Metadata {
    pub name: String,
    // pub group: String,
}

#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Provider {
    pub name: String,
}
