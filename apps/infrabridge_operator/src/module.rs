use kube::CustomResource;
use serde::{Deserialize, Serialize};
use schemars::JsonSchema;

#[derive(CustomResource, Deserialize, Serialize, Clone, Debug, JsonSchema)]
#[kube(group = "infrabridge.io", version = "v1", kind = "Module", namespaced)]
pub struct ModuleSpec {
    #[serde(rename = "moduleName")]
    pub module_name: String,
    environment: String,
    parameters: Vec<Parameter>,
}

#[derive(Debug, Serialize, Deserialize, Clone, JsonSchema)]
pub struct Parameter {
    name: String,
    #[serde(rename = "type")]
    type_: String,
}
