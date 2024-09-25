use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Variable {
    pub name: String,
    #[serde(rename = "type")]
    pub _type: String,
    pub default: Option<serde_json::Value>,
    pub description: Option<String>,
    pub required: Option<bool>,
    pub sensitive: Option<bool>,
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct Validation {
    pub expression: String,
    pub message: String,
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct Output {
    pub name: String,
    pub value: String,
    pub description: String,
}
