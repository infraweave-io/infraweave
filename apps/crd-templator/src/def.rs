use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct Module {
    pub apiVersion: String,
    pub kind: String,
    pub metadata: Metadata,
    pub spec: ModuleSpec, // Make spec public
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Metadata {
    pub name: String, // It's a good practice to also make Metadata fields public if you'll need to access them
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ModuleSpec {
    pub moduleName: String, // Make ModuleSpec fields public
    pub environment: String,
    pub parameters: Vec<Parameter>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Parameter {
    pub name: String, // Make Parameter fields public
    #[serde(rename = "type")] // Correctly map 'type' from the input to 'type_' in Rust
    pub type_: String,
}
