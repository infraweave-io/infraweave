use core::panic;
use std::{future::Future, pin::Pin, process::exit, sync::Arc};

use async_trait::async_trait;
use env_aws::AwsCloudProvider;
use env_azure::AzureCloudProvider;
use env_defs::{
    CloudProvider, CloudProviderCommon, Dependent, DeploymentResp, EventData,
    GenericFunctionResponse, InfraChangeRecord, JobStatus, LogData, ModuleResp, NotificationData,
    PolicyResp, ProjectData, ProviderResp,
};
use serde_json::Value;

use crate::logic::{
    insert_event, insert_infra_change_record, publish_notification, publish_policy, read_logs,
    set_deployment, OCIRegistryProvider, PROJECT_ID, REGION,
};

#[derive(Clone)]
pub struct GenericCloudHandler {
    provider: Arc<dyn CloudProvider>,
    oci_registry: Option<OCIRegistryProvider>,
}

impl GenericCloudHandler {
    /// Factory method that picks the right provider based on an environment variable.
    pub async fn default() -> Self {
        Self::factory(PROJECT_ID.get().cloned(), REGION.get().cloned(), None).await
    }
    pub async fn custom(function_endpoint: &str) -> Self {
        Self::factory(
            PROJECT_ID.get().cloned(),
            REGION.get().cloned(),
            Some(function_endpoint.to_string()),
        )
        .await
    }
    pub async fn region(region: &str) -> Self {
        Self::factory(PROJECT_ID.get().cloned(), Some(region.to_string()), None).await
    }
    pub async fn workload(project_id: &str, region: &str) -> Self {
        Self::factory(Some(project_id.to_string()), Some(region.to_string()), None).await
    }
    pub async fn central() -> Self {
        Self::factory(Some("central".to_string()), REGION.get().cloned(), None).await
    }
    pub fn get_oci_client(&self) -> Option<&OCIRegistryProvider> {
        self.oci_registry.as_ref()
    }

    async fn factory(
        project_id: Option<String>,
        region: Option<String>,
        function_endpoint: Option<String>,
    ) -> Self {
        let provider: Arc<dyn CloudProvider> = match provider_name().as_str() {
            "aws" => {
                let region = match region {
                    Some(r) => r,
                    None => env_aws::get_region().await,
                };
                let project_id = match project_id {
                    Some(p) => p,
                    None => match env_aws::get_project_id().await {
                        Ok(p) => p,
                        Err(e) => {
                            eprintln!("Error initializing: {:?}", e);
                            exit(1);
                        }
                    },
                };
                Arc::new(AwsCloudProvider {
                    project_id: project_id.to_string(),
                    region: region.to_string(),
                    function_endpoint,
                })
            }
            "azure" => {
                let project_id = match project_id {
                    Some(p) => p,
                    None => match env_azure::get_project_id().await {
                        Ok(p) => p,
                        Err(e) => {
                            eprintln!("Error initializing: {:?}", e);
                            exit(1);
                        }
                    },
                };
                let region = match region {
                    Some(r) => r,
                    None => env_azure::get_region().await,
                };
                Arc::new(AzureCloudProvider {
                    project_id: project_id.to_string(),
                    region: region.to_string(),
                    function_endpoint,
                })
            }
            "none" => Arc::new(super::NoCloudProvider::default()),
            _ => panic!("Unsupported provider: {}", provider_name()),
        };
        let oci_registry = match std::env::var("OCI_REGISTRY_URI") {
            Ok(oci_registry) => {
                let oci_username = match std::env::var("OCI_REGISTRY_USERNAME") {
                    Ok(username) => Some(username),
                    Err(_) => None,
                };
                let oci_password = match std::env::var("OCI_REGISTRY_PASSWORD") {
                    Ok(password) => Some(password),
                    Err(_) => None,
                };
                Some(OCIRegistryProvider::new(
                    oci_registry,
                    oci_username,
                    oci_password,
                ))
            }
            Err(_) => None,
        };
        Self {
            provider,
            oci_registry,
        }
    }

    pub async fn copy_with_region(&self, new_region: &str) -> Self {
        let project_id = self.get_project_id().to_string();
        let function_endpoint = self.get_function_endpoint();
        Self::factory(
            Some(project_id),
            Some(new_region.to_string()),
            function_endpoint,
        )
        .await
    }
}

#[async_trait]
impl CloudProviderCommon for GenericCloudHandler {
    async fn set_deployment(
        &self,
        deployment: &DeploymentResp,
        is_plan: bool,
    ) -> Result<(), anyhow::Error> {
        set_deployment(self, deployment, is_plan).await
    }
    async fn insert_infra_change_record(
        &self,
        infra_change_record: InfraChangeRecord,
        plan_output_raw: &str,
    ) -> Result<String, anyhow::Error> {
        insert_infra_change_record(self, infra_change_record, plan_output_raw).await
    }
    async fn insert_event(&self, event: EventData) -> Result<String, anyhow::Error> {
        insert_event(self, event).await
    }
    async fn publish_notification(
        &self,
        notification: NotificationData,
    ) -> Result<String, anyhow::Error> {
        publish_notification(self, notification).await
    }
    async fn read_logs(&self, job_id: &str) -> Result<Vec<LogData>, anyhow::Error> {
        read_logs(self, PROJECT_ID.get().unwrap(), job_id).await
    }
    async fn publish_policy(
        &self,
        manifest_path: &str,
        environment: &str,
    ) -> Result<(), anyhow::Error> {
        publish_policy(self, manifest_path, environment).await
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
    fn get_function_endpoint(&self) -> Option<String> {
        self.provider.get_function_endpoint()
    }
    fn get_cloud_provider(&self) -> &str {
        self.provider.get_cloud_provider()
    }
    fn get_backend_provider(&self) -> &str {
        self.provider.get_backend_provider()
    }
    fn get_storage_basepath(&self) -> String {
        self.provider.get_storage_basepath()
    }
    async fn get_backend_provider_arguments(
        &self,
        environment: &str,
        deployment_id: &str,
    ) -> serde_json::Value {
        self.provider
            .get_backend_provider_arguments(environment, deployment_id)
            .await
    }
    async fn set_backend(
        &self,
        exec: &mut tokio::process::Command,
        deployment_id: &str,
        environment: &str,
    ) {
        self.provider
            .set_backend(exec, deployment_id, environment)
            .await
    }
    async fn get_current_job_id(&self) -> Result<String, anyhow::Error> {
        self.provider.get_current_job_id().await
    }
    async fn get_project_map(&self) -> Result<Value, anyhow::Error> {
        self.provider.get_project_map().await
    }
    async fn get_all_regions(&self) -> Result<Vec<String>, anyhow::Error> {
        self.provider.get_all_regions().await
    }
    async fn run_function(
        &self,
        payload: &Value,
    ) -> Result<GenericFunctionResponse, anyhow::Error> {
        self.provider.run_function(payload).await
    }
    fn read_db_generic(
        &self,
        table: &str,
        query: &Value,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<Value>, anyhow::Error>> + Send>> {
        self.provider.read_db_generic(table, query)
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
    async fn get_latest_provider_version(
        &self,
        provider: &str,
    ) -> Result<Option<ProviderResp>, anyhow::Error> {
        self.provider.get_latest_provider_version(provider).await
    }
    async fn generate_presigned_url(
        &self,
        key: &str,
        bucket: &str,
    ) -> Result<String, anyhow::Error> {
        self.provider.generate_presigned_url(key, bucket).await
    }
    async fn get_all_latest_module(&self, track: &str) -> Result<Vec<ModuleResp>, anyhow::Error> {
        self.provider.get_all_latest_module(track).await
    }
    async fn get_all_latest_stack(&self, track: &str) -> Result<Vec<ModuleResp>, anyhow::Error> {
        self.provider.get_all_latest_stack(track).await
    }
    async fn get_all_latest_provider(&self) -> Result<Vec<ProviderResp>, anyhow::Error> {
        self.provider.get_all_latest_provider().await
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
    async fn get_job_status(&self, job_id: &str) -> Result<Option<JobStatus>, anyhow::Error> {
        self.provider.get_job_status(job_id).await
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
    async fn get_environment_variables(&self) -> Result<serde_json::Value, anyhow::Error> {
        self.provider.get_environment_variables().await
    }
    async fn download_state_file(
        &self,
        environment: &str,
        deployment_id: &str,
        output: Option<String>,
    ) -> Result<(), anyhow::Error> {
        self.provider
            .download_state_file(environment, deployment_id, output)
            .await
    }
}

impl GenericCloudHandler {
    pub async fn get_change_record_json(
        &self,
        environment: &str,
        deployment_id: &str,
        job_id: &str,
        command: &str,
    ) -> Result<Value, anyhow::Error> {
        let change_type = command.to_uppercase();

        let change_record = self
            .get_change_record(environment, deployment_id, job_id, &change_type)
            .await?;

        let presigned_url = self
            .generate_presigned_url(&change_record.plan_raw_json_key, "change_records")
            .await?;

        let client = reqwest::Client::new();
        let json_content = client
            .get(&presigned_url)
            .send()
            .await
            .map_err(|e| anyhow::anyhow!("Failed to download Terraform JSON output: {}", e))?
            .text()
            .await
            .map_err(|e| anyhow::anyhow!("Failed to read Terraform JSON response: {}", e))?;

        let terraform_json: Value = serde_json::from_str(&json_content)
            .map_err(|e| anyhow::anyhow!("Failed to parse Terraform JSON output: {}", e))?;

        Ok(terraform_json)
    }
}

pub async fn initialize_project_id_and_region() -> String {
    if crate::logic::PROJECT_ID.get().is_none() {
        let project_id = match std::env::var("TEST_MODE") {
            Ok(_) => "test-mode".to_string(),
            Err(_) => GenericCloudHandler::default()
                .await
                .provider
                .get_project_id()
                .to_string(),
        };
        eprintln!("Project ID: {}", &project_id);
        crate::logic::PROJECT_ID
            .set(project_id.clone())
            .expect("Failed to set PROJECT_ID");
    }
    if crate::logic::REGION.get().is_none() {
        let region = match std::env::var("TEST_MODE") {
            Ok(_) => "us-west-2".to_string(),
            Err(_) => GenericCloudHandler::default()
                .await
                .provider
                .get_region()
                .to_string(),
        };
        eprintln!("Region: {}", &region);
        crate::logic::REGION
            .set(region)
            .expect("Failed to set REGION");
    }
    crate::logic::PROJECT_ID.get().unwrap().clone()
}

pub async fn get_current_identity() -> String {
    let current_identity = env_aws::get_user_id().await.unwrap();
    eprintln!("Current identity: {}", &current_identity);
    current_identity
}

fn provider_name() -> String {
    std::env::var("PROVIDER").unwrap_or_else(|_| "aws".into()) // TODO: don't use fallback
}
