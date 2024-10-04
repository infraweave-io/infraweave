use core::panic;

use async_trait::async_trait;
use env_defs::{
    ApiInfraPayload, Dependent, DeploymentResp, EnvironmentResp, EventData, ModuleResp, PolicyResp, ResourceResp
};

#[async_trait]
pub trait ModuleEnvironmentHandler {
    async fn publish_module(
        &self,
        manifest_path: &String,
        environment: &String,
    ) -> Result<(), anyhow::Error>;
    async fn list_module(&self, environment: &String) -> Result<Vec<ModuleResp>, anyhow::Error>;
    async fn get_module_download_url(&self, s3_key: &String) -> Result<String, anyhow::Error>;
    async fn insert_event(&self, event: EventData) -> Result<String, anyhow::Error>;
    async fn get_events(&self, deployment_id: &String) -> Result<Vec<EventData>, anyhow::Error>;
    async fn set_deployment(&self, deployment: DeploymentResp) -> Result<String, anyhow::Error>;
    async fn get_module_version(
        &self,
        module: &String,
        version: &String,
    ) -> Result<ModuleResp, anyhow::Error>;
    async fn get_latest_module_version(
        &self,
        module: &String,
        environment: &String,
    ) -> anyhow::Result<ModuleResp>;
    async fn mutate_infra(&self, payload: ApiInfraPayload) -> Result<String, anyhow::Error>;
    async fn list_environments(&self) -> Result<Vec<EnvironmentResp>, anyhow::Error>;
    async fn list_deployments(&self) -> Result<Vec<DeploymentResp>, anyhow::Error>;
    async fn list_resources(&self, region: &str) -> Result<Vec<ResourceResp>, anyhow::Error>;
    async fn describe_deployment_id(
        &self,
        deployment_id: &str,
        environment: &str,
    ) -> anyhow::Result<(DeploymentResp, Vec<Dependent>)>;
    async fn read_logs(&self, job_id: &str) -> Result<String, anyhow::Error>;
    async fn bootstrap_environment(&self, local: bool, plan: bool) -> Result<(), anyhow::Error>;
    async fn bootstrap_teardown_environment(&self, local: bool) -> Result<(), anyhow::Error>;
    async fn list_policy(&self, environment: &str) -> Result<Vec<PolicyResp>, anyhow::Error>;
    async fn publish_policy(
        &self,
        manifest_path: &String,
        environment: &String,
    ) -> Result<(), anyhow::Error>;
    async fn get_policy_version(
        &self,
        policy: &String,
        environment: &String,
        version: &String,
    ) -> Result<PolicyResp, anyhow::Error>;
    async fn get_policy_download_url(&self, s3_key: &String) -> Result<String, anyhow::Error>;
}

pub struct AwsHandler;
pub struct AzureHandler;

#[async_trait]
impl ModuleEnvironmentHandler for AwsHandler {
    async fn publish_module(
        &self,
        manifest_path: &String,
        environment: &String,
    ) -> Result<(), anyhow::Error> {
        env_aws::publish_module(manifest_path, environment).await
    }
    async fn list_module(&self, environment: &String) -> Result<Vec<ModuleResp>, anyhow::Error> {
        env_aws::list_module(environment).await
    }
    async fn get_module_download_url(&self, s3_key: &String) -> Result<String, anyhow::Error> {
        env_aws::get_module_download_url(s3_key).await
    }
    async fn insert_event(&self, event: EventData) -> Result<String, anyhow::Error> {
        env_aws::insert_event(event).await
    }
    async fn get_events(&self, deployment_id: &String) -> Result<Vec<EventData>, anyhow::Error> {
        env_aws::get_events(deployment_id).await
    }
    async fn set_deployment(&self, deployment: DeploymentResp) -> Result<String, anyhow::Error> {
        env_aws::set_deployment(deployment).await
    }
    async fn get_module_version(
        &self,
        module: &String,
        version: &String,
    ) -> Result<ModuleResp, anyhow::Error> {
        env_aws::get_module_version(module, version).await
    }
    async fn get_latest_module_version(
        &self,
        module: &String,
        environment: &String,
    ) -> anyhow::Result<ModuleResp> {
        env_aws::get_latest_module_version(module, environment).await
    }
    async fn mutate_infra(&self, payload: ApiInfraPayload) -> Result<String, anyhow::Error> {
        env_aws::mutate_infra(payload).await
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
        environment: &str,
    ) -> anyhow::Result<(DeploymentResp, Vec<Dependent>)> {
        env_aws::describe_deployment_id(deployment_id, environment).await
    }
    async fn read_logs(&self, deployment_id: &str) -> Result<String, anyhow::Error> {
        env_aws::read_logs(deployment_id).await
    }
    async fn bootstrap_environment(&self, local: bool, plan: bool) -> Result<(), anyhow::Error> {
        env_aws::bootstrap_environment(local, plan).await
    }
    async fn bootstrap_teardown_environment(&self, local: bool) -> Result<(), anyhow::Error> {
        env_aws::bootstrap_teardown_environment(local).await
    }
    async fn list_policy(&self, environment: &str) -> Result<Vec<PolicyResp>, anyhow::Error> {
        env_aws::list_policy(environment).await
    }
    async fn publish_policy(
        &self,
        manifest_path: &String,
        environment: &String,
    ) -> Result<(), anyhow::Error> {
        env_aws::publish_policy(manifest_path, environment).await
    }
    async fn get_policy_version(
        &self,
        policy: &String,
        environment: &String,
        version: &String,
    ) -> Result<PolicyResp, anyhow::Error> {
        env_aws::get_policy_version(policy, environment, version).await
    }
    async fn get_policy_download_url(&self, s3_key: &String) -> Result<String, anyhow::Error> {
        env_aws::get_policy_download_url(s3_key).await
    }
}

#[async_trait]
impl ModuleEnvironmentHandler for AzureHandler {
    async fn publish_module(
        &self,
        manifest_path: &String,
        environment: &String,
    ) -> Result<(), anyhow::Error> {
        env_azure::publish_module(manifest_path, environment).await
    }
    async fn list_module(&self, environment: &String) -> Result<Vec<ModuleResp>, anyhow::Error> {
        env_azure::list_module(environment).await
    }
    async fn get_module_download_url(&self, s3_key: &String) -> Result<String, anyhow::Error> {
        env_azure::get_module_download_url(s3_key).await
    }
    async fn insert_event(&self, event: EventData) -> Result<String, anyhow::Error> {
        panic!("Not implemented for Azure");
    }
    async fn get_events(&self, deployment_id: &String) -> Result<Vec<EventData>, anyhow::Error> {
        panic!("Not implemented for Azure");
    }
    async fn set_deployment(&self, deployment: DeploymentResp) -> Result<String, anyhow::Error> {
        panic!("Not implemented for Azure");
    }
    async fn get_module_version(
        &self,
        module: &String,
        version: &String,
    ) -> Result<ModuleResp, anyhow::Error> {
        env_azure::get_module_version(module, version).await
    }
    async fn get_latest_module_version(
        &self,
        module: &String,
        environment: &String,
    ) -> anyhow::Result<ModuleResp> {
        // env_azure::get_latest_module_version(module, environment).await
        panic!("Not implemented for Azure")
    }
    async fn mutate_infra(&self, payload: ApiInfraPayload) -> Result<String, anyhow::Error> {
        env_azure::mutate_infra(payload).await
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
        environment: &str
    ) -> anyhow::Result<(DeploymentResp, Vec<Dependent>)> {
        // env_azure::describe_deployment_id(deployment_id, region).await
        panic!("Not implemented for Azure")
    }
    async fn read_logs(&self, deployment_id: &str) -> Result<String, anyhow::Error> {
        // env_azure::read_logs(deployment_id).await
        panic!("Not implemented for Azure")
    }
    async fn bootstrap_environment(&self, local: bool, plan: bool) -> Result<(), anyhow::Error> {
        env_azure::bootstrap_environment(local).await
    }
    async fn bootstrap_teardown_environment(&self, local: bool) -> Result<(), anyhow::Error> {
        env_azure::bootstrap_teardown_environment(local).await
    }
    async fn list_policy(&self, environment: &str) -> Result<Vec<PolicyResp>, anyhow::Error> {
        // env_azure::list_policy(environment).await
        panic!("Not implemented for Azure")
    }
    async fn publish_policy(
        &self,
        manifest_path: &String,
        environment: &String,
    ) -> Result<(), anyhow::Error> {
        // env_azure::publish_policy(manifest_path, environment).await
        panic!("Not implemented for Azure")
    }
    async fn get_policy_version(
        &self,
        policy: &String,
        environment: &String,
        version: &String,
    ) -> Result<PolicyResp, anyhow::Error> {
        // env_azure::get_policy_version(policy, version).await
        panic!("Not implemented for Azure")
    }
    async fn get_policy_download_url(&self, s3_key: &String) -> Result<String, anyhow::Error> {
        // env_azure::get_policy_download_url(s3_key).await
        panic!("Not implemented for Azure")
    }
}
