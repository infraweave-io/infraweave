use env_defs::{
    CloudProvider, CloudProviderCommon, Dependent, DeploymentResp, EventData,
    GenericFunctionResponse, InfraChangeRecord, LogData, ModuleResp, NotificationData, PolicyResp,
    ProjectData,
};
use serde_json::Value;
use std::{future::Future, pin::Pin};

use async_trait::async_trait;

#[derive(Clone, Default)]
pub struct NoCloudProvider {
    pub project_id: String,
    pub region: String,
    pub function_endpoint: Option<String>,
}

#[async_trait]
impl CloudProviderCommon for NoCloudProvider {
    async fn set_deployment(
        &self,
        _deployment: &DeploymentResp,
        _is_plan: bool,
    ) -> Result<(), anyhow::Error> {
        Ok(())
    }

    async fn insert_event(&self, _event: EventData) -> Result<String, anyhow::Error> {
        Ok(String::new())
    }

    async fn publish_notification(
        &self,
        _notification: NotificationData,
    ) -> Result<String, anyhow::Error> {
        Ok(String::new())
    }

    async fn insert_infra_change_record(
        &self,
        _record: InfraChangeRecord,
        _raw: &str,
    ) -> Result<String, anyhow::Error> {
        Ok(String::new())
    }

    async fn read_logs(&self, _job_id: &str) -> Result<Vec<LogData>, anyhow::Error> {
        Ok(vec![])
    }

    async fn publish_policy(
        &self,
        _manifest_path: &str,
        _environment: &str,
    ) -> Result<(), anyhow::Error> {
        Ok(())
    }
}

#[async_trait]
impl CloudProvider for NoCloudProvider {
    fn get_project_id(&self) -> &str {
        ""
    }

    async fn get_user_id(&self) -> Result<String, anyhow::Error> {
        Ok(String::new())
    }

    fn get_region(&self) -> &str {
        ""
    }

    fn get_function_endpoint(&self) -> Option<String> {
        None
    }

    fn get_cloud_provider(&self) -> &str {
        ""
    }

    fn get_backend_provider(&self) -> &str {
        ""
    }

    fn get_storage_basepath(&self) -> String {
        String::new()
    }

    async fn set_backend(
        &self,
        _cmd: &mut tokio::process::Command,
        _deployment_id: &str,
        _environment: &str,
    ) {
    }

    async fn get_current_job_id(&self) -> Result<String, anyhow::Error> {
        Ok(String::new())
    }

    async fn get_project_map(&self) -> Result<Value, anyhow::Error> {
        Ok(Value::Null)
    }

    async fn get_all_regions(&self) -> Result<Vec<String>, anyhow::Error> {
        Ok(vec![])
    }

    async fn run_function(
        &self,
        _payload: &Value,
    ) -> Result<GenericFunctionResponse, anyhow::Error> {
        Ok(GenericFunctionResponse {
            payload: Value::Null,
        })
    }

    fn read_db_generic(
        &self,
        _table: &str,
        _query: &Value,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<Value>, anyhow::Error>> + Send>> {
        Box::pin(async { Ok(vec![]) })
    }

    async fn get_latest_module_version(
        &self,
        _module: &str,
        _track: &str,
    ) -> Result<Option<ModuleResp>, anyhow::Error> {
        Ok(None)
    }

    async fn get_latest_stack_version(
        &self,
        _stack: &str,
        _track: &str,
    ) -> Result<Option<ModuleResp>, anyhow::Error> {
        Ok(None)
    }

    async fn generate_presigned_url(
        &self,
        _key: &str,
        _bucket: &str,
    ) -> Result<String, anyhow::Error> {
        Ok(String::new())
    }

    async fn get_all_latest_module(&self, _track: &str) -> Result<Vec<ModuleResp>, anyhow::Error> {
        Ok(vec![])
    }

    async fn get_all_latest_stack(&self, _track: &str) -> Result<Vec<ModuleResp>, anyhow::Error> {
        Ok(vec![])
    }

    async fn get_all_module_versions(
        &self,
        _module: &str,
        _track: &str,
    ) -> Result<Vec<ModuleResp>, anyhow::Error> {
        Ok(vec![])
    }

    async fn get_all_stack_versions(
        &self,
        _stack: &str,
        _track: &str,
    ) -> Result<Vec<ModuleResp>, anyhow::Error> {
        Ok(vec![])
    }

    async fn get_module_version(
        &self,
        _module: &str,
        _track: &str,
        _version: &str,
    ) -> Result<Option<ModuleResp>, anyhow::Error> {
        Ok(None)
    }

    async fn get_stack_version(
        &self,
        _module: &str,
        _track: &str,
        _version: &str,
    ) -> Result<Option<ModuleResp>, anyhow::Error> {
        Ok(None)
    }

    async fn get_all_deployments(
        &self,
        _environment: &str,
    ) -> Result<Vec<DeploymentResp>, anyhow::Error> {
        Ok(vec![])
    }

    async fn get_deployment_and_dependents(
        &self,
        _deployment_id: &str,
        _environment: &str,
        _include_dependents: bool,
    ) -> Result<(Option<DeploymentResp>, Vec<Dependent>), anyhow::Error> {
        Ok((None, vec![]))
    }

    async fn get_deployment(
        &self,
        _deployment_id: &str,
        _environment: &str,
        _include_deleted: bool,
    ) -> Result<Option<DeploymentResp>, anyhow::Error> {
        Ok(None)
    }

    async fn get_deployments_using_module(
        &self,
        _module: &str,
        _environment: &str,
    ) -> Result<Vec<DeploymentResp>, anyhow::Error> {
        Ok(vec![])
    }

    async fn get_plan_deployment(
        &self,
        _deployment_id: &str,
        _environment: &str,
        _job_id: &str,
    ) -> Result<Option<DeploymentResp>, anyhow::Error> {
        Ok(None)
    }

    async fn get_dependents(
        &self,
        _deployment_id: &str,
        _environment: &str,
    ) -> Result<Vec<Dependent>, anyhow::Error> {
        Ok(vec![])
    }

    async fn get_deployments_to_driftcheck(&self) -> Result<Vec<DeploymentResp>, anyhow::Error> {
        Ok(vec![])
    }

    async fn get_all_projects(&self) -> Result<Vec<ProjectData>, anyhow::Error> {
        Ok(vec![])
    }

    async fn get_current_project(&self) -> Result<ProjectData, anyhow::Error> {
        Err(anyhow::anyhow!("no current project"))
    }

    async fn get_events(
        &self,
        _deployment_id: &str,
        _environment: &str,
    ) -> Result<Vec<EventData>, anyhow::Error> {
        Ok(vec![])
    }

    async fn get_all_events_between(
        &self,
        _start_epoch: u128,
        _end_epoch: u128,
    ) -> Result<Vec<EventData>, anyhow::Error> {
        Ok(vec![])
    }

    async fn get_change_record(
        &self,
        _environment: &str,
        _deployment_id: &str,
        _job_id: &str,
        _change_type: &str,
    ) -> Result<InfraChangeRecord, anyhow::Error> {
        Err(anyhow::anyhow!("no change record"))
    }

    async fn get_newest_policy_version(
        &self,
        _policy: &str,
        _environment: &str,
    ) -> Result<PolicyResp, anyhow::Error> {
        Err(anyhow::anyhow!("no newest policy version"))
    }

    async fn get_all_policies(&self, _environment: &str) -> Result<Vec<PolicyResp>, anyhow::Error> {
        Ok(vec![])
    }

    async fn get_policy(
        &self,
        _policy: &str,
        _environment: &str,
        _version: &str,
    ) -> Result<PolicyResp, anyhow::Error> {
        Err(anyhow::anyhow!("no policy"))
    }

    async fn get_policy_download_url(&self, _key: &str) -> Result<String, anyhow::Error> {
        Ok(String::new())
    }
}
