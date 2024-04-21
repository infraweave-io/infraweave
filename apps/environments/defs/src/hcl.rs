use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct Variable {
    pub name: String,
    #[serde(rename = "type")]
    pub _type: String,
    pub default: String,
    pub description: String,
    pub nullable: bool,
    pub sensitive: bool,
    // pub validation: Validation,
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
