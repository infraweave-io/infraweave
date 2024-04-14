use async_trait::async_trait;
use env_defs::{DeploymentResp, EnvironmentResp, ModuleResp, ResourceResp};

#[async_trait]
pub trait ModuleEnvironmentHandler {
    async fn publish_module(
        &self,
        manifest_path: &String,
        environment: &String,
        description: &String,
        reference: &String,
    ) -> Result<(), anyhow::Error>;
    async fn list_module(&self, environment: &String) -> Result<Vec<ModuleResp>, anyhow::Error>;
    async fn get_module_version(
        &self,
        module: &String,
        version: &String,
    ) -> Result<ModuleResp, anyhow::Error>;
    async fn mutate_infra(
        &self,
        event: String,
        module: String,
        name: String,
        environment: String,
        deployment_id: String,
        spec: serde_json::Value,
        annotations: serde_json::Value,
    ) -> Result<String, anyhow::Error>;
    async fn list_environments(&self) -> Result<Vec<EnvironmentResp>, anyhow::Error>;
    async fn list_deployments(&self) -> Result<Vec<DeploymentResp>, anyhow::Error>;
    async fn list_resources(&self, region: &str) -> Result<Vec<ResourceResp>, anyhow::Error>;
    async fn describe_deployment_id(
        &self,
        deployment_id: &str,
        region: &str,
    ) -> Result<DeploymentResp, anyhow::Error>;
    async fn bootstrap_environment(
        &self,
        region: &String,
        local: bool,
    ) -> Result<(), anyhow::Error>;
    async fn bootstrap_teardown_environment(
        &self,
        region: &String,
        local: bool,
    ) -> Result<(), anyhow::Error>;
}

pub struct AwsHandler;
pub struct AzureHandler;

#[async_trait]
impl ModuleEnvironmentHandler for AwsHandler {
    async fn publish_module(
        &self,
        manifest_path: &String,
        environment: &String,
        description: &String,
        reference: &String,
    ) -> Result<(), anyhow::Error> {
        env_aws::publish_module(manifest_path, environment, description, reference).await
    }
    async fn list_module(&self, environment: &String) -> Result<Vec<ModuleResp>, anyhow::Error> {
        env_aws::list_module(environment).await
    }
    async fn get_module_version(
        &self,
        module: &String,
        version: &String,
    ) -> Result<ModuleResp, anyhow::Error> {
        env_aws::get_module_version(module, version).await
    }
    async fn mutate_infra(
        &self,
        event: String,
        module: String,
        name: String,
        environment: String,
        deployment_id: String,
        spec: serde_json::Value,
        annotations: serde_json::Value,
    ) -> Result<String, anyhow::Error> {
        env_aws::mutate_infra(
            event,
            module,
            name,
            environment,
            deployment_id,
            spec,
            annotations,
        )
        .await
    }
    async fn list_environments(&self) -> Result<Vec<EnvironmentResp>, anyhow::Error> {
        env_aws::list_environments().await
    }
    async fn list_deployments(&self) -> Result<Vec<DeploymentResp>, anyhow::Error> {
        env_aws::list_deployments().await
    }
    async fn list_resources(&self, region: &str) -> Result<Vec<ResourceResp>, anyhow::Error> {
        env_aws::list_resources(region).await
    }
    async fn describe_deployment_id(
        &self,
        deployment_id: &str,
        region: &str,
    ) -> Result<DeploymentResp, anyhow::Error> {
        env_aws::describe_deployment_id(deployment_id, region).await
    }
    async fn bootstrap_environment(
        &self,
        region: &String,
        local: bool,
    ) -> Result<(), anyhow::Error> {
        env_aws::bootstrap_environment(region, local).await
    }
    async fn bootstrap_teardown_environment(
        &self,
        region: &String,
        local: bool,
    ) -> Result<(), anyhow::Error> {
        env_aws::bootstrap_teardown_environment(region, local).await
    }
}

#[async_trait]
impl ModuleEnvironmentHandler for AzureHandler {
    async fn publish_module(
        &self,
        manifest_path: &String,
        environment: &String,
        description: &String,
        reference: &String,
    ) -> Result<(), anyhow::Error> {
        env_azure::publish_module(manifest_path, environment, description, reference).await
    }
    async fn list_module(&self, environment: &String) -> Result<Vec<ModuleResp>, anyhow::Error> {
        env_azure::list_module(environment).await
    }
    async fn get_module_version(
        &self,
        module: &String,
        version: &String,
    ) -> Result<ModuleResp, anyhow::Error> {
        env_azure::get_module_version(module, version).await
    }
    async fn mutate_infra(
        &self,
        event: String,
        module: String,
        name: String,
        environment: String,
        deployment_id: String,
        spec: serde_json::Value,
        annotations: serde_json::Value,
    ) -> Result<String, anyhow::Error> {
        env_azure::mutate_infra(
            event,
            module,
            name,
            environment,
            deployment_id,
            spec,
            annotations,
        )
        .await
    }
    async fn list_environments(&self) -> Result<Vec<EnvironmentResp>, anyhow::Error> {
        env_azure::list_environments().await
    }
    async fn list_deployments(&self) -> Result<Vec<DeploymentResp>, anyhow::Error> {
        // env_azure::list_deployments(region).await
        panic!("Not implemented for Azure")
    }
    async fn list_resources(&self, region: &str) -> Result<Vec<ResourceResp>, anyhow::Error> {
        // env_azure::list_resources().await
        panic!("Not implemented for Azure")
    }
    async fn describe_deployment_id(
        &self,
        deployment_id: &str,
        region: &str,
    ) -> Result<DeploymentResp, anyhow::Error> {
        // env_azure::describe_deployment_id(deployment_id, region).await
        panic!("Not implemented for Azure")
    }
    async fn bootstrap_environment(
        &self,
        region: &String,
        local: bool,
    ) -> Result<(), anyhow::Error> {
        env_azure::bootstrap_environment(region, local).await
    }
    async fn bootstrap_teardown_environment(
        &self,
        region: &String,
        local: bool,
    ) -> Result<(), anyhow::Error> {
        env_azure::bootstrap_teardown_environment(region, local).await
    }
}
