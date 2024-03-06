use kube::CustomResource;
use serde::{Deserialize, Serialize};
use schemars::JsonSchema;

#[derive(CustomResource, Deserialize, Serialize, Clone, Debug, JsonSchema)]
#[kube(group = "infrabridge.io", version = "v1", kind = "Module", namespaced)] //, status = "ModuleStatus")] // Note: https://github.com/kube-rs/kube/blob/main/examples/crd_derive.rs#L25C97-L26C110
pub struct ModuleSpec {
    #[serde(rename = "moduleName")]
    pub module_name: String,
    environment: String,
    version: String,
    parameters: Vec<Parameter>,
}

#[derive(Debug, Serialize, Deserialize, Clone, JsonSchema)]
pub struct Parameter {
    name: String,
    #[serde(rename = "type")]
    type_: String,
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

// #[derive(Deserialize, Serialize, Clone, Debug, Default, JsonSchema)]
// pub struct ModuleStatus {
//     health: String,
//     approxCostUSD: String,
// }