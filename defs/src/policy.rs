use serde::{Deserialize, Serialize};

pub fn get_policy_identifier(policy: &str, environment: &str) -> String {
    format!("{}::{}", environment, policy)
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct PolicyResp {
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

pub fn deserialize_policy_manifest<'de, D>(deserializer: D) -> Result<PolicyManifest, D::Error>
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
pub struct PolicyManifest {
    pub metadata: Metadata,
    #[serde(rename = "apiVersion")]
    pub api_version: String,
    pub kind: String,
    pub spec: PolicySpec,
}

// This struct represents the actual spec part of the manifest
#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct PolicySpec {
    #[serde(rename = "policyName")]
    pub policy_name: String,
    pub version: String,
    pub description: String,
    pub reference: String,
    pub data: serde_json::Value,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Metadata {
    pub name: String,
    // pub group: String,
}

#[derive(Deserialize, Clone, Debug, Serialize)]
pub struct PolicyResult {
    pub policy: String,
    #[serde(default)]
    pub version: String,
    pub environment: String,
    pub description: String,
    pub policy_name: String,
    pub failed: bool,
    pub violations: serde_json::Value,
}