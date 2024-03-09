use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct Module {
    pub apiVersion: String,
    pub kind: String,
    pub metadata: ModuleMetadata,
    pub spec: ModuleSpec,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ModuleMetadata {
    pub group: String,
    pub name: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ModuleSpec {
    pub moduleName: String,
    pub environment: String,
    pub parameters: Vec<Parameter>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Parameter {
    pub name: String,
    #[serde(rename = "type")] // Map 'type' from the input to 'type_' in Rust
    pub type_: String,
}
