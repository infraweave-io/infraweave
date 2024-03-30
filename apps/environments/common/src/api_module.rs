use async_trait::async_trait;
use env_defs::ModuleResp;

#[async_trait]
pub trait ModuleEnvironmentHandler {
    async fn publish_module(&self, manifest_path: &String, environment: &String, description: &String, reference: &String) -> Result<(), anyhow::Error>;
    async fn list_module(&self, environment: &String) -> Result<Vec<ModuleResp>, anyhow::Error>;
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
}

#[async_trait]
impl ModuleEnvironmentHandler for AzureHandler {
    async fn publish_module(&self, manifest_path: &String, environment: &String, description: &String, reference: &String) -> Result<(), anyhow::Error> {
        env_azure::publish_module(manifest_path, environment, description, reference).await
    }
    async fn list_module(&self, environment: &String) -> Result<Vec<ModuleResp>, anyhow::Error> {
        env_azure::list_module(environment).await
    }
}
