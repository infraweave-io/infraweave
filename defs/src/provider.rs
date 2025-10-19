use std::{future::Future, pin::Pin};

use crate::{
    deployment::JobStatus, Dependent, DeploymentResp, EventData, GenericFunctionResponse,
    InfraChangeRecord, LogData, ModuleResp, NotificationData, PolicyResp, ProjectData,
};

use async_trait::async_trait;
use serde_json::Value;

#[async_trait]
pub trait CloudProviderCommon: Send + Sync {
    async fn set_deployment(
        &self,
        deployment: &DeploymentResp,
        is_plan: bool,
    ) -> Result<(), anyhow::Error>;
    async fn insert_event(&self, event: EventData) -> Result<String, anyhow::Error>;
    async fn publish_notification(
        &self,
        notification: NotificationData,
    ) -> Result<String, anyhow::Error>;
    async fn insert_infra_change_record(
        &self,
        infra_change_record: InfraChangeRecord,
        plan_output_raw: &str,
    ) -> Result<String, anyhow::Error>;
    async fn read_logs(&self, job_id: &str) -> Result<Vec<LogData>, anyhow::Error>;
    async fn publish_policy(
        &self,
        manifest_path: &str,
        environment: &str,
    ) -> Result<(), anyhow::Error>;
}

#[async_trait]
pub trait CloudProvider: Send + Sync {
    fn get_project_id(&self) -> &str;
    async fn get_user_id(&self) -> Result<String, anyhow::Error>;
    fn get_region(&self) -> &str;
    fn get_function_endpoint(&self) -> Option<String>;
    fn get_cloud_provider(&self) -> &str;
    fn get_backend_provider(&self) -> &str;
    fn get_storage_basepath(&self) -> String;
    async fn get_backend_provider_arguments(
        &self,
        environment: &str,
        deployment_id: &str,
    ) -> serde_json::Value;
    async fn set_backend(
        &self,
        exec: &mut tokio::process::Command,
        deployment_id: &str,
        environment: &str,
    );
    async fn get_current_job_id(&self) -> Result<String, anyhow::Error>;
    async fn get_project_map(&self) -> Result<Value, anyhow::Error>;
    async fn get_all_regions(&self) -> Result<Vec<String>, anyhow::Error>;
    // Function
    async fn run_function(&self, payload: &Value)
        -> Result<GenericFunctionResponse, anyhow::Error>;
    fn read_db_generic(
        &self,
        table: &str,
        query: &Value,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<Value>, anyhow::Error>> + Send>>;
    // Module + stack
    async fn get_latest_module_version(
        &self,
        module: &str,
        track: &str,
    ) -> Result<Option<ModuleResp>, anyhow::Error>;
    async fn get_latest_stack_version(
        &self,
        stack: &str,
        track: &str,
    ) -> Result<Option<ModuleResp>, anyhow::Error>;
    async fn generate_presigned_url(
        &self,
        key: &str,
        bucket: &str,
    ) -> Result<String, anyhow::Error>;
    async fn get_all_latest_module(&self, track: &str) -> Result<Vec<ModuleResp>, anyhow::Error>;
    async fn get_all_latest_stack(&self, track: &str) -> Result<Vec<ModuleResp>, anyhow::Error>;
    async fn get_all_module_versions(
        &self,
        module: &str,
        track: &str,
    ) -> Result<Vec<ModuleResp>, anyhow::Error>;
    async fn get_all_stack_versions(
        &self,
        stack: &str,
        track: &str,
    ) -> Result<Vec<ModuleResp>, anyhow::Error>;
    async fn get_module_version(
        &self,
        module: &str,
        track: &str,
        version: &str,
    ) -> Result<Option<ModuleResp>, anyhow::Error>;
    async fn get_stack_version(
        &self,
        module: &str,
        track: &str,
        version: &str,
    ) -> Result<Option<ModuleResp>, anyhow::Error>;
    // Deployment
    async fn get_all_deployments(
        &self,
        environment: &str,
    ) -> Result<Vec<DeploymentResp>, anyhow::Error>;
    async fn get_deployment_and_dependents(
        &self,
        deployment_id: &str,
        environment: &str,
        include_deleted: bool,
    ) -> Result<(Option<DeploymentResp>, Vec<Dependent>), anyhow::Error>;
    async fn get_deployment(
        &self,
        deployment_id: &str,
        environment: &str,
        include_deleted: bool,
    ) -> Result<Option<DeploymentResp>, anyhow::Error>;
    async fn get_job_status(&self, job_id: &str) -> Result<Option<JobStatus>, anyhow::Error>;
    async fn get_deployments_using_module(
        &self,
        module: &str,
        environment: &str,
    ) -> Result<Vec<DeploymentResp>, anyhow::Error>;
    async fn get_plan_deployment(
        &self,
        deployment_id: &str,
        environment: &str,
        job_id: &str,
    ) -> Result<Option<DeploymentResp>, anyhow::Error>;
    async fn get_dependents(
        &self,
        deployment_id: &str,
        environment: &str,
    ) -> Result<Vec<Dependent>, anyhow::Error>;
    async fn get_deployments_to_driftcheck(&self) -> Result<Vec<DeploymentResp>, anyhow::Error>;
    async fn get_all_projects(&self) -> Result<Vec<ProjectData>, anyhow::Error>;
    async fn get_current_project(&self) -> Result<ProjectData, anyhow::Error>;
    // Event
    async fn get_events(
        &self,
        deployment_id: &str,
        environment: &str,
    ) -> Result<Vec<EventData>, anyhow::Error>;
    async fn get_all_events_between(
        &self,
        start_epoch: u128,
        end_epoch: u128,
    ) -> Result<Vec<EventData>, anyhow::Error>;
    // Change record
    async fn get_change_record(
        &self,
        environment: &str,
        deployment_id: &str,
        job_id: &str,
        change_type: &str,
    ) -> Result<InfraChangeRecord, anyhow::Error>;
    // Policy
    async fn get_newest_policy_version(
        &self,
        policy: &str,
        environment: &str,
    ) -> Result<PolicyResp, anyhow::Error>;
    async fn get_all_policies(&self, environment: &str) -> Result<Vec<PolicyResp>, anyhow::Error>;
    async fn get_policy_download_url(&self, key: &str) -> Result<String, anyhow::Error>;
    async fn get_policy(
        &self,
        policy: &str,
        environment: &str,
        version: &str,
    ) -> Result<PolicyResp, anyhow::Error>;
    async fn get_environment_variables(&self) -> Result<serde_json::Value, anyhow::Error>;
}
