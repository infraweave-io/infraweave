use core::panic;
use std::sync::Arc;

use async_trait::async_trait;
use env_aws::AwsCloudProvider;
use env_azure::AzureCloudProvider;
use env_defs::{
    CloudProvider, CloudProviderCommon, Dependent, DeploymentResp, EventData,
    GenericFunctionResponse, InfraChangeRecord, LogData, ModuleResp, PolicyResp, ProjectData,
};
use serde_json::Value;

use crate::logic::{
    insert_event, insert_infra_change_record, publish_policy, read_logs, set_deployment,
    set_project, PROJECT_ID, REGION,
};

#[derive(Clone)]
pub struct GenericCloudHandler {
    provider: Arc<dyn CloudProvider>,
}

impl GenericCloudHandler {
    /// Factory method that picks the right provider based on an environment variable.
    pub async fn default() -> Self {
        Self::factory(PROJECT_ID.get().unwrap(), REGION.get().unwrap(), None).await
    }
    pub async fn custom(function_endpoint: &str) -> Self {
        Self::factory(
            &PROJECT_ID.get().unwrap(),
            &REGION.get().unwrap(),
            Some(function_endpoint.to_string()),
        )
        .await
    }
    pub async fn workload(project_id: &str, region: &str) -> Self {
        Self::factory(project_id, region, None).await
    }
    pub async fn central() -> Self {
        Self::factory("central", &REGION.get().unwrap(), None).await
    }

    async fn factory(project_id: &str, region: &str, function_endpoint: Option<String>) -> Self {
        let provider_name = std::env::var("PROVIDER").unwrap_or_else(|_| "aws".into());
        let provider: Arc<dyn CloudProvider> = match provider_name.as_str() {
            "aws" => Arc::new(AwsCloudProvider {
                project_id: project_id.to_string(),
                region: region.to_string(),
                function_endpoint: function_endpoint,
            }),
            "azure" => Arc::new(AzureCloudProvider {
                project_id: project_id.to_string(),
                region: region.to_string(),
                function_endpoint: function_endpoint,
            }),
            _ => panic!("Unsupported provider: {}", provider_name),
        };
        Self { provider }
    }
}

#[async_trait]
impl CloudProviderCommon for GenericCloudHandler {
    async fn set_deployment(
        &self,
        deployment: &DeploymentResp,
        is_plan: bool,
    ) -> Result<(), anyhow::Error> {
        set_deployment(&self, deployment, is_plan).await
    }
    async fn set_project(&self, project: &ProjectData) -> Result<(), anyhow::Error> {
        set_project(&self, project).await
    }
    async fn insert_infra_change_record(
        &self,
        infra_change_record: InfraChangeRecord,
        plan_output_raw: &str,
    ) -> Result<String, anyhow::Error> {
        insert_infra_change_record(&self, infra_change_record, plan_output_raw).await
    }
    async fn insert_event(&self, event: EventData) -> Result<String, anyhow::Error> {
        insert_event(&self, event).await
    }
    async fn read_logs(&self, job_id: &str) -> Result<Vec<LogData>, anyhow::Error> {
        read_logs(&self, PROJECT_ID.get().unwrap(), job_id).await
    }
    async fn publish_policy(
        &self,
        manifest_path: &str,
        environment: &str,
    ) -> Result<(), anyhow::Error> {
        publish_policy(&self, manifest_path, environment).await
    }
}

#[async_trait]
impl CloudProvider for GenericCloudHandler {
    fn get_project_id(&self) -> &str {
        self.provider.get_project_id()
    }
    async fn get_user_id(&self) -> Result<String, anyhow::Error> {
        self.provider.get_user_id().await
    }
    fn get_region(&self) -> &str {
        self.provider.get_region()
    }
    fn get_cloud_provider(&self) -> &str {
        self.provider.get_cloud_provider()
    }
    async fn run_function(
        &self,
        payload: &Value,
    ) -> Result<GenericFunctionResponse, anyhow::Error> {
        self.provider.run_function(payload).await
    }
    async fn get_latest_module_version(
        &self,
        module: &str,
        track: &str,
    ) -> Result<Option<ModuleResp>, anyhow::Error> {
        self.provider.get_latest_module_version(module, track).await
    }
    async fn get_latest_stack_version(
        &self,
        stack: &str,
        track: &str,
    ) -> Result<Option<ModuleResp>, anyhow::Error> {
        self.provider.get_latest_stack_version(stack, track).await
    }
    async fn generate_presigned_url(&self, key: &str) -> Result<String, anyhow::Error> {
        self.provider.generate_presigned_url(key).await
    }
    async fn get_all_latest_module(&self, track: &str) -> Result<Vec<ModuleResp>, anyhow::Error> {
        self.provider.get_all_latest_module(track).await
    }
    async fn get_all_latest_stack(&self, track: &str) -> Result<Vec<ModuleResp>, anyhow::Error> {
        self.provider.get_all_latest_stack(track).await
    }
    async fn get_all_module_versions(
        &self,
        module: &str,
        track: &str,
    ) -> Result<Vec<ModuleResp>, anyhow::Error> {
        self.provider.get_all_module_versions(module, track).await
    }
    async fn get_all_stack_versions(
        &self,
        stack: &str,
        track: &str,
    ) -> Result<Vec<ModuleResp>, anyhow::Error> {
        self.provider.get_all_stack_versions(stack, track).await
    }
    async fn get_module_version(
        &self,
        module: &str,
        track: &str,
        version: &str,
    ) -> Result<Option<ModuleResp>, anyhow::Error> {
        self.provider
            .get_module_version(module, track, version)
            .await
    }
    async fn get_stack_version(
        &self,
        stack: &str,
        track: &str,
        version: &str,
    ) -> Result<Option<ModuleResp>, anyhow::Error> {
        self.provider.get_stack_version(stack, track, version).await
    }
    // Deployment
    async fn get_all_deployments(
        &self,
        environment: &str,
    ) -> Result<Vec<DeploymentResp>, anyhow::Error> {
        self.provider.get_all_deployments(environment).await
    }
    async fn get_deployment_and_dependents(
        &self,
        deployment_id: &str,
        environment: &str,
        include_deleted: bool,
    ) -> Result<(Option<DeploymentResp>, Vec<Dependent>), anyhow::Error> {
        self.provider
            .get_deployment_and_dependents(deployment_id, environment, include_deleted)
            .await
    }
    async fn get_deployment(
        &self,
        deployment_id: &str,
        environment: &str,
        include_deleted: bool,
    ) -> Result<Option<DeploymentResp>, anyhow::Error> {
        self.provider
            .get_deployment(deployment_id, environment, include_deleted)
            .await
    }
    async fn get_deployments_using_module(
        &self,
        module: &str,
        environment: &str,
    ) -> Result<Vec<DeploymentResp>, anyhow::Error> {
        self.provider
            .get_deployments_using_module(module, environment)
            .await
    }
    async fn get_plan_deployment(
        &self,
        deployment_id: &str,
        environment: &str,
        job_id: &str,
    ) -> Result<Option<DeploymentResp>, anyhow::Error> {
        self.provider
            .get_plan_deployment(deployment_id, environment, job_id)
            .await
    }
    async fn get_dependents(
        &self,
        deployment_id: &str,
        environment: &str,
    ) -> Result<Vec<Dependent>, anyhow::Error> {
        self.provider
            .get_dependents(deployment_id, environment)
            .await
    }
    async fn get_deployments_to_driftcheck(&self) -> Result<Vec<DeploymentResp>, anyhow::Error> {
        self.provider.get_deployments_to_driftcheck().await
    }
    async fn get_all_projects(&self) -> Result<Vec<ProjectData>, anyhow::Error> {
        self.provider.get_all_projects().await
    }
    async fn get_current_project(&self) -> Result<ProjectData, anyhow::Error> {
        self.provider.get_current_project().await
    }
    // Event
    async fn get_events(
        &self,
        deployment_id: &str,
        environment: &str,
    ) -> Result<Vec<EventData>, anyhow::Error> {
        self.provider.get_events(deployment_id, environment).await
    }
    async fn get_all_events_between(
        &self,
        start_epoch: u128,
        end_epoch: u128,
    ) -> Result<Vec<EventData>, anyhow::Error> {
        self.provider
            .get_all_events_between(start_epoch, end_epoch)
            .await
    }
    // Change record
    async fn get_change_record(
        &self,
        environment: &str,
        deployment_id: &str,
        job_id: &str,
        change_type: &str,
    ) -> Result<InfraChangeRecord, anyhow::Error> {
        self.provider
            .get_change_record(environment, deployment_id, job_id, change_type)
            .await
    }
    // Policy
    async fn get_newest_policy_version(
        &self,
        policy: &str,
        environment: &str,
    ) -> Result<PolicyResp, anyhow::Error> {
        self.provider
            .get_newest_policy_version(policy, environment)
            .await
    }
    async fn get_all_policies(&self, environment: &str) -> Result<Vec<PolicyResp>, anyhow::Error> {
        self.provider.get_all_policies(environment).await
    }
    async fn get_policy_download_url(&self, key: &str) -> Result<String, anyhow::Error> {
        self.provider.get_policy_download_url(key).await
    }
    async fn get_policy(
        &self,
        policy: &str,
        environment: &str,
        version: &str,
    ) -> Result<PolicyResp, anyhow::Error> {
        self.provider.get_policy(policy, environment, version).await
    }
}

pub async fn initialize_project_id_and_region() -> String {
    // if true {
    //     crate::logic::PROJECT_ID.set("3f9---732".to_string()).expect("Failed to set PROJECT_ID");
    //     crate::logic::REGION.set("West Europe".to_string()).expect("Failed to set REGION");
    // }
    if crate::logic::PROJECT_ID.get().is_none() {
        let account_id = match std::env::var("TEST_MODE") {
            Ok(_) => "test-mode".to_string(),
            Err(_) => env_aws::get_project_id().await.unwrap(),
        };
        println!("Account ID: {}", &account_id);
        crate::logic::PROJECT_ID
            .set(account_id.clone())
            .expect("Failed to set PROJECT_ID");
    }
    if crate::logic::REGION.get().is_none() {
        let region = match std::env::var("TEST_MODE") {
            Ok(_) => "us-west-2".to_string(),
            Err(_) => env_aws::get_region().await,
        };
        println!("Region: {}", &region);
        crate::logic::REGION
            .set(region)
            .expect("Failed to set REGION");
    }
    crate::logic::PROJECT_ID.get().unwrap().clone()
}

pub async fn get_current_identity() -> String {
    let current_identity = env_aws::get_user_id().await.unwrap();
    println!("Current identity: {}", &current_identity);
    current_identity
}
