use async_trait::async_trait;

#[async_trait]
pub trait ModulePublisher {
    async fn publish_module(&self, manifest_path: &String, environment: &String, description: &String, reference: &String) -> Result<(), anyhow::Error>;
}

pub struct AwsPublisher;
pub struct AzurePublisher;

#[async_trait]
impl ModulePublisher for AwsPublisher {
    async fn publish_module(&self, manifest_path: &String, environment: &String, description: &String, reference: &String) -> Result<(), anyhow::Error> {
        env_aws::publish_module(manifest_path, environment, description, reference).await
    }
}

#[async_trait]
impl ModulePublisher for AzurePublisher {
    async fn publish_module(&self, manifest_path: &String, environment: &String, description: &String, reference: &String) -> Result<(), anyhow::Error> {
        env_azure::publish_module(manifest_path, environment, description, reference).await
    }
}