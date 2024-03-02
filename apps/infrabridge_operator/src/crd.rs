use kube::{CustomResource};
use serde::{Serialize, Deserialize};
use schemars::JsonSchema;

#[derive(CustomResource, Serialize, Deserialize, Clone, Debug, JsonSchema)]
#[kube(group = "infrabridge.io", version = "v1", kind = "GeneralCRD", namespaced)]
pub struct GeneralCRDSpec {
    pub api_version: String,
    pub kind: String,
    pub metadata: Metadata,
    // Assuming spec is similar across all kinds; adjust as needed.
    pub spec: GeneralSpec,
}

#[derive(Debug, Serialize, Deserialize, Clone, JsonSchema)]
pub struct Metadata {
    pub name: String,
    // Include other necessary metadata fields
}

#[derive(Debug, Serialize, Deserialize, Clone, JsonSchema)]
pub struct GeneralSpec {
    // Assuming all CRDs have parameters that can vary; model accordingly.
    pub parameters: Vec<Parameter>,
}

#[derive(Debug, Serialize, Deserialize, Clone, JsonSchema)]
pub struct Parameter {
    pub name: String,
    pub type_: String,
    // Potentially include additional fields to represent parameter values.
}
