use serde_json::Value;

pub struct GenericFunctionResponse {
    pub payload: Value,
    // pub error: Option<String>,
    // pub return_code: i32,
}

pub trait GenericCloudConfig: Clone {
    fn get_function_endpoint(&self) -> Option<String>;
    fn default() -> Self;
    fn custom(endpoint: &str) -> Self;
}
