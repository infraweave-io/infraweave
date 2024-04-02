use async_trait::async_trait;
use env_defs::{ModuleResp, EnvironmentResp};

#[async_trait]
pub trait ModuleEnvironmentHandler {
    async fn publish_module(&self, manifest_path: &String, environment: &String, description: &String, reference: &String) -> Result<(), anyhow::Error>;
    async fn list_module(&self, environment: &String) -> Result<Vec<ModuleResp>, anyhow::Error>;
    async fn get_module_version(&self, module: &String, version: &String) -> Result<ModuleResp, anyhow::Error>;
    async fn mutate_infra(&self, event: String, module: String, name: String, environment: String, deployment_id: String, spec: serde_json::Value, annotations: serde_json::Value) -> Result<String, anyhow::Error>;
    async fn list_environments(&self) -> Result<Vec<EnvironmentResp>, anyhow::Error>;
}

pub struct AwsHandler;
pub struct AzureHandler;

#[async_trait]
impl ModuleEnvironmentHandler for AwsHandler {
    async fn publish_module(&self, manifest_path: &String, environment: &String, description: &String, reference: &String) -> Result<(), anyhow::Error> {
        env_aws::publish_module(manifest_path, environment, description, reference).await
    }
    async fn list_module(&self, environment: &String) -> Result<Vec<ModuleResp>, anyhow::Error> {
        env_aws::list_module(environment).await
    }
    async fn get_module_version(&self, module: &String, version: &String) -> Result<ModuleResp, anyhow::Error> {
        env_aws::get_module_version(module, version).await
    }
    async fn mutate_infra(&self, event: String, module: String, name: String, environment: String, deployment_id: String, spec: serde_json::Value, annotations: serde_json::Value) -> Result<String, anyhow::Error> {
        env_aws::mutate_infra(event, module, name, environment, deployment_id, spec, annotations).await
    }
    async fn list_environments(&self) -> Result<Vec<EnvironmentResp>, anyhow::Error> {
        env_aws::list_environments().await
    }
}

#[async_trait]
impl ModuleEnvironmentHandler for AzureHandler {
    async fn publish_module(&self, manifest_path: &String, environment: &String, description: &String, reference: &String) -> Result<(), anyhow::Error> {
        env_azure::publish_module(manifest_path, environment, description, reference).await
    }
    async fn list_module(&self, environment: &String) -> Result<Vec<ModuleResp>, anyhow::Error> {
        env_azure::list_module(environment).await
    }
    async fn get_module_version(&self, module: &String, version: &String) -> Result<ModuleResp, anyhow::Error> {
        env_azure::get_module_version(module, version).await
    }
    async fn mutate_infra(&self, event: String, module: String, name: String, environment: String, deployment_id: String, spec: serde_json::Value, annotations: serde_json::Value) -> Result<String, anyhow::Error> {
        env_azure::mutate_infra(event, module, name, environment, deployment_id, spec, annotations).await
    }
    async fn list_environments(&self) -> Result<Vec<EnvironmentResp>, anyhow::Error> {
        env_azure::list_environments().await
    }
}
