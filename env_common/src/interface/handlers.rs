use core::panic;

use async_trait::async_trait;
use env_defs::{
    ApiInfraPayload, Dependent, DeploymentResp, EventData, InfraChangeRecord, ModuleResp, PolicyResp
};
use serde_json::Value;

use crate::{
    get_module_download_url, list_modules, list_stacks, logic::{
        get_all_deployments, get_all_module_versions, get_all_policies, get_all_stack_versions, get_change_record, get_deployment_and_dependents, get_deployments_using_module, get_events, get_latest_module_version, get_module_version, get_plan_deployment, get_policy, get_policy_download_url, get_stack_version, insert_event, insert_infra_change_record, mutate_infra, precheck_module, publish_policy, publish_stack, read_logs, set_deployment
    }, publish_module 
};

#[async_trait]
pub trait ModuleEnvironmentHandler {
    async fn publish_module(
        &self,
        manifest_path: &String,
        track: &String,
    ) -> Result<(), anyhow::Error>;
    async fn precheck_module(
        &self,
        manifest_path: &String,
        track: &String,
    ) -> Result<(), anyhow::Error>;
    async fn publish_stack(
        &self,
        manifest_path: &String,
        track: &String,
    ) -> Result<(), anyhow::Error>;
    async fn list_modules(&self, track: &String) -> Result<Vec<ModuleResp>, anyhow::Error>;
    async fn list_stacks(&self, track: &String) -> Result<Vec<ModuleResp>, anyhow::Error>;
    async fn get_module_download_url(&self, s3_key: &String) -> Result<String, anyhow::Error>;
    async fn insert_event(&self, event: EventData) -> Result<String, anyhow::Error>;
    async fn get_events(&self, deployment_id: &String) -> Result<Vec<EventData>, anyhow::Error>;
    async fn set_deployment(&self, deployment: DeploymentResp, is_plan: bool) -> Result<String, anyhow::Error>;
    async fn insert_infra_change_record(&self, infra_change_record: InfraChangeRecord, plan_output_raw: &str) -> Result<String, anyhow::Error>;
    async fn get_module_version(
        &self,
        module: &String,
        track: &String,
        version: &String,
    ) -> Result<Option<ModuleResp>, anyhow::Error>;
    async fn get_stack_version(
        &self,
        stack: &String,
        track: &String,
        version: &String,
    ) -> Result<Option<ModuleResp>, anyhow::Error>;
    async fn get_all_module_versions(&self, module: &str, track: &str) -> Result<Vec<ModuleResp>, anyhow::Error>;
    async fn get_all_stack_versions(&self, stack: &str, track: &str) -> Result<Vec<ModuleResp>, anyhow::Error>;
    async fn get_latest_module_version(
        &self,
        module: &String,
        track: &String,
    ) -> Result<Option<ModuleResp>, anyhow::Error>;
    async fn mutate_infra(&self, payload: ApiInfraPayload) -> Result<Value, anyhow::Error>;
    async fn list_deployments(&self) -> Result<Vec<DeploymentResp>, anyhow::Error>;
    async fn get_deployments_using_module(&self, module: &str, environment: &str) -> anyhow::Result<Vec<DeploymentResp>>;
    async fn describe_deployment_id(
        &self,
        deployment_id: &str,
        environment: &str,
    ) -> anyhow::Result<(DeploymentResp, Vec<Dependent>)>;
    async fn describe_plan_job(&self, deployment_id: &str, environment: &str, job_id: &str) -> anyhow::Result<DeploymentResp>;
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
    async fn get_change_record(&self, environment: &str, deployment_id: &str, job_id: &str, change_type: &str) -> Result<InfraChangeRecord, anyhow::Error>;
}

pub struct AwsHandler;
pub struct AzureHandler;

#[async_trait]
impl ModuleEnvironmentHandler for AwsHandler {
    async fn publish_module(
        &self,
        manifest_path: &String,
        track: &String,
    ) -> Result<(), anyhow::Error> {
        publish_module(manifest_path, track).await
    }
    async fn precheck_module(
        &self,
        manifest_path: &String,
        track: &String,
    ) -> Result<(), anyhow::Error> {
        precheck_module(manifest_path, track).await
    }
    async fn publish_stack(
        &self,
        manifest_path: &String,
        track: &String,
    ) -> Result<(), anyhow::Error> {
        publish_stack(manifest_path, track).await
    }
    async fn list_modules(&self, track: &String) -> Result<Vec<ModuleResp>, anyhow::Error> {
        list_modules(track).await
    }
    async fn list_stacks(&self, track: &String) -> Result<Vec<ModuleResp>, anyhow::Error> {
        list_stacks(track).await
    }
    async fn get_module_download_url(&self, s3_key: &String) -> Result<String, anyhow::Error> {
        get_module_download_url(s3_key).await
    }
    async fn insert_event(&self, event: EventData) -> Result<String, anyhow::Error> {
        insert_event(event).await
    }
    async fn get_events(&self, deployment_id: &String) -> Result<Vec<EventData>, anyhow::Error> {
        get_events(deployment_id).await
    }
    async fn set_deployment(&self, deployment: DeploymentResp, is_plan: bool) -> Result<String, anyhow::Error> {
        set_deployment(deployment,is_plan).await?;
        Ok("".to_string())
    }
    async fn insert_infra_change_record(&self, infra_change_record: InfraChangeRecord, plan_output_raw: &str) -> Result<String, anyhow::Error> {
        insert_infra_change_record(infra_change_record, &plan_output_raw).await
    }
    async fn get_module_version(
        &self,
        module: &String,
        track: &String,
        version: &String,
    ) -> Result<Option<ModuleResp>, anyhow::Error> {
        get_module_version(module, track, version).await
    }
    async fn get_stack_version(
        &self,
        stack: &String,
        track: &String,
        version: &String,
    ) -> Result<Option<ModuleResp>, anyhow::Error> {
        get_stack_version(stack, track, version).await
    }
    async fn get_all_module_versions(&self, module: &str, track: &str) -> Result<Vec<ModuleResp>, anyhow::Error> {
        get_all_module_versions(module, track).await
    }
    async fn get_all_stack_versions(&self, stack: &str, track: &str) -> Result<Vec<ModuleResp>, anyhow::Error> {
        get_all_stack_versions(stack, track).await
    }
    async fn get_latest_module_version(
        &self,
        module: &String,
        track: &String,
    ) -> Result<Option<ModuleResp>, anyhow::Error> {
        get_latest_module_version(module, track).await
    }
    async fn mutate_infra(&self, payload: ApiInfraPayload) -> Result<Value, anyhow::Error> {
        match mutate_infra(payload).await {
            Ok(response) => Ok(response.payload),
            Err(e) => Err(e),
        }
    }
    async fn list_deployments(&self) -> Result<Vec<DeploymentResp>, anyhow::Error> {
        let environment = "";
        get_all_deployments(environment).await
    }
    async fn get_deployments_using_module(&self, module: &str, environment: &str) -> anyhow::Result<Vec<DeploymentResp>> {
        get_deployments_using_module(module, environment).await
    }
    async fn describe_deployment_id(
        &self,
        deployment_id: &str,
        environment: &str,
    ) -> anyhow::Result<(DeploymentResp, Vec<Dependent>)> {
        match get_deployment_and_dependents(deployment_id, environment, false).await {
            Ok((deployment, dependents)) => match deployment {
                Some(deployment) => Ok((deployment, dependents)),
                None => panic!("Deployment could not describe since it was not found"),
            },
            Err(e) => Err(e),
        }
    }
    async fn describe_plan_job(&self, deployment_id: &str, environment: &str, job_id: &str) -> anyhow::Result<DeploymentResp> {
        match get_plan_deployment(deployment_id, environment, job_id).await {
            Ok(deployment) => match deployment {
                Some(deployment) => Ok(deployment),
                None => panic!("Deployment plan could not describe since it was not found"),
            },
            Err(e) => Err(e),
        }
    }
    async fn read_logs(&self, job_id: &str) -> Result<String, anyhow::Error> {
        match read_logs(job_id).await {
            Ok(logs) => {
                let mut log_str = String::new();
                for log in logs {
                    log_str.push_str(&format!("{}\n", log.message));
                }
                Ok(log_str)
            },
            Err(e) => Err(e),
        }
    }
    async fn bootstrap_environment(&self, local: bool, plan: bool) -> Result<(), anyhow::Error> {
        env_aws::bootstrap_environment(local, plan).await
    }
    async fn bootstrap_teardown_environment(&self, local: bool) -> Result<(), anyhow::Error> {
        env_aws::bootstrap_teardown_environment(local).await
    }
    async fn list_policy(&self, environment: &str) -> Result<Vec<PolicyResp>, anyhow::Error> {
        get_all_policies(environment).await
    }
    async fn publish_policy(
        &self,
        manifest_path: &String,
        environment: &String,
    ) -> Result<(), anyhow::Error> {
        publish_policy(manifest_path, environment).await
    }
    async fn get_policy_version(
        &self,
        policy: &String,
        environment: &String,
        version: &String,
    ) -> Result<PolicyResp, anyhow::Error> {
        get_policy(policy, environment, version).await
    }
    async fn get_policy_download_url(&self, s3_key: &String) -> Result<String, anyhow::Error> {
        get_policy_download_url(s3_key).await
    }
    async fn get_change_record(&self, environment: &str, deployment_id: &str, job_id: &str, change_type: &str) -> Result<InfraChangeRecord, anyhow::Error> {
        get_change_record(environment, deployment_id, job_id, change_type).await
    }
}

#[async_trait]
impl ModuleEnvironmentHandler for AzureHandler {
    async fn publish_module(
        &self,
        manifest_path: &String,
        track: &String,
    ) -> Result<(), anyhow::Error> {
        env_azure::publish_module(manifest_path, track).await
    }
    async fn precheck_module(
        &self,
        manifest_path: &String,
        track: &String,
    ) -> Result<(), anyhow::Error> {
        panic!("Not implemented for Azure");
    }
    async fn publish_stack(
        &self,
        manifest_path: &String,
        track: &String,
    ) -> Result<(), anyhow::Error> {
        panic!("Not implemented for Azure");
    }
    async fn list_modules(&self, track: &String) -> Result<Vec<ModuleResp>, anyhow::Error> {
        env_azure::list_module(track).await
    }
    async fn list_stacks(&self, track: &String) -> Result<Vec<ModuleResp>, anyhow::Error> {
        panic!("Not implemented for Azure");
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
    async fn set_deployment(&self, deployment: DeploymentResp, is_plan: bool) -> Result<String, anyhow::Error> {
        panic!("Not implemented for Azure");
    }
    async fn insert_infra_change_record(&self, infra_change_record: InfraChangeRecord, plan_output_raw: &str) -> Result<String, anyhow::Error> {
        panic!("Not implemented for Azure");
    }
    async fn get_module_version(
        &self,
        module: &String,
        track: &String,
        version: &String,
    ) -> Result<Option<ModuleResp>, anyhow::Error> {
        panic!("Not implemented for Azure")
    }
    async fn get_stack_version(
        &self,
        stack: &String,
        track: &String,
        version: &String,
    ) -> Result<Option<ModuleResp>, anyhow::Error> {
        panic!("Not implemented for Azure")
    }
    async fn get_all_module_versions(&self, module: &str, track: &str) -> Result<Vec<ModuleResp>, anyhow::Error> {
        panic!("Not implemented for Azure")
    }
    async fn get_all_stack_versions(&self, stack: &str, track: &str) -> Result<Vec<ModuleResp>, anyhow::Error> {
        panic!("Not implemented for Azure")
    }
    async fn get_latest_module_version(
        &self,
        module: &String,
        track: &String,
    ) -> Result<Option<ModuleResp>, anyhow::Error> {
        // env_azure::get_latest_module_version(module, track).await
        panic!("Not implemented for Azure")
    }
    async fn mutate_infra(&self, payload: ApiInfraPayload) -> Result<Value, anyhow::Error> {
        env_azure::mutate_infra(payload).await
    }
    async fn list_deployments(&self) -> Result<Vec<DeploymentResp>, anyhow::Error> {
        // env_azure::list_deployments(region).await
        panic!("Not implemented for Azure")
    }
    async fn get_deployments_using_module(&self, module: &str, environment: &str) -> anyhow::Result<Vec<DeploymentResp>> {
        // env_azure::get_deployments_using_module(module).await
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
    async fn describe_plan_job(&self, deployment_id: &str, environment: &str, job_id: &str) -> anyhow::Result<DeploymentResp> {
        // env_azure::describe_plan_job(deployment_id, region).await
        panic!("Not implemented for Azure")
    }
    async fn read_logs(&self, job_id: &str) -> Result<String, anyhow::Error> {
        // env_azure::read_logs(job_id).await
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
    async fn get_change_record(&self, environment: &str, deployment_id: &str, job_id: &str, change_type: &str) -> Result<InfraChangeRecord, anyhow::Error> {
        panic!("Not implemented for Azure")
    }
}
