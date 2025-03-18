use async_trait::async_trait;
use env_defs::{
    CloudProvider, Dependent, DeploymentResp, EventData, GenericFunctionResponse,
    InfraChangeRecord, ModuleResp, PolicyResp, ProjectData,
};
use env_utils::{
    _get_change_records, _get_dependents, _get_deployment, _get_deployment_and_dependents,
    _get_deployments, _get_events, _get_module_optional, _get_modules, _get_policies, _get_policy,
    get_projects,
};
use serde_json::Value;
use std::{future::Future, pin::Pin};

#[derive(Clone)]
pub struct AzureCloudProvider {
    pub project_id: String,
    pub region: String,
    pub function_endpoint: Option<String>,
}

#[async_trait]
impl CloudProvider for AzureCloudProvider {
    fn get_project_id(&self) -> &str {
        &self.project_id
    }
    async fn get_user_id(&self) -> Result<String, anyhow::Error> {
        crate::get_user_id().await
    }
    fn get_region(&self) -> &str {
        &self.region
    }
    fn get_function_endpoint(&self) -> Option<String> {
        self.function_endpoint.clone()
    }
    fn get_cloud_provider(&self) -> &str {
        "azure"
    }
    fn get_backend_provider(&self) -> &str {
        "azurerm"
    }
    async fn set_backend(
        &self,
        exec: &mut tokio::process::Command,
        deployment_id: &str,
        environment: &str,
    ) {
        crate::set_backend(exec, deployment_id, environment).await;
    }
    async fn get_current_job_id(&self) -> Result<String, anyhow::Error> {
        crate::get_current_job_id().await
    }
    async fn get_project_map(&self) -> Result<Value, anyhow::Error> {
        self.read_db_generic("config", &crate::get_project_map_query())
            .await
            .map(|mut items| items.pop().expect("No project map found"))
    }

    async fn get_all_regions(&self) -> Result<Vec<String>, anyhow::Error> {
        self.read_db_generic("config", &crate::get_all_regions_query())
            .await
            .map(|mut items| items.pop().expect("No all_regions item found"))
            .map(|item| {
                item.get("data")
                    .expect("No data field in response")
                    .get("regions")
                    .expect("No regions field in response")
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|region| region.as_str().unwrap().to_string())
                    .collect()
            })
    }
    async fn run_function(&self, items: &Value) -> Result<GenericFunctionResponse, anyhow::Error> {
        crate::run_function(
            &self.function_endpoint,
            items,
            &self.project_id,
            &self.region,
        )
        .await
    }
    fn read_db_generic(
        &self,
        table: &str,
        query: &Value,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<Value>, anyhow::Error>> + Send>> {
        let table = table.to_string();
        let query = query.clone();
        let function_endpoint = self.function_endpoint.clone();
        let project_id = self.project_id.clone();
        let region = self.region.clone();
        Box::pin(async move {
            match crate::read_db(&function_endpoint, &table, &query, &project_id, &region).await {
                Ok(response) => {
                    let items = response.payload.as_array().unwrap_or(&vec![]).clone();
                    Ok(items.clone())
                }
                Err(e) => Err(e),
            }
        })
    }
    async fn get_latest_module_version(
        &self,
        module: &str,
        track: &str,
    ) -> Result<Option<ModuleResp>, anyhow::Error> {
        _get_module_optional(self, crate::get_latest_module_version_query(module, track)).await
    }
    async fn get_latest_stack_version(
        &self,
        stack: &str,
        track: &str,
    ) -> Result<Option<ModuleResp>, anyhow::Error> {
        _get_module_optional(self, crate::get_latest_stack_version_query(stack, track)).await
    }
    async fn generate_presigned_url(&self, key: &str) -> Result<String, anyhow::Error> {
        match crate::run_function(
            &self.function_endpoint,
            &crate::get_generate_presigned_url_query(key, "modules"),
            &self.project_id,
            &self.region,
        )
        .await
        {
            Ok(response) => match response.payload.get("url") {
                Some(url) => Ok(url.as_str().unwrap().to_string()),
                None => Err(anyhow::anyhow!("Presigned url not found in response")),
            },
            Err(e) => Err(e),
        }
    }
    async fn get_all_latest_module(&self, track: &str) -> Result<Vec<ModuleResp>, anyhow::Error> {
        _get_modules(self, crate::get_all_latest_modules_query(track)).await
    }
    async fn get_all_latest_stack(&self, track: &str) -> Result<Vec<ModuleResp>, anyhow::Error> {
        _get_modules(self, crate::get_all_latest_stacks_query(track)).await
    }
    async fn get_all_module_versions(
        &self,
        module: &str,
        track: &str,
    ) -> Result<Vec<ModuleResp>, anyhow::Error> {
        _get_modules(self, crate::get_all_module_versions_query(module, track)).await
    }
    async fn get_all_stack_versions(
        &self,
        stack: &str,
        track: &str,
    ) -> Result<Vec<ModuleResp>, anyhow::Error> {
        _get_modules(self, crate::get_all_stack_versions_query(stack, track)).await
    }
    async fn get_module_version(
        &self,
        module: &str,
        track: &str,
        version: &str,
    ) -> Result<Option<ModuleResp>, anyhow::Error> {
        _get_module_optional(
            self,
            crate::get_module_version_query(module, track, version),
        )
        .await
    }
    async fn get_stack_version(
        &self,
        stack: &str,
        track: &str,
        version: &str,
    ) -> Result<Option<ModuleResp>, anyhow::Error> {
        _get_module_optional(self, crate::get_stack_version_query(stack, track, version)).await
    }
    // Deployment
    async fn get_all_deployments(
        &self,
        environment: &str,
    ) -> Result<Vec<DeploymentResp>, anyhow::Error> {
        _get_deployments(
            self,
            crate::get_all_deployments_query(&self.project_id, &self.region, environment),
        )
        .await
    }
    async fn get_deployment_and_dependents(
        &self,
        deployment_id: &str,
        environment: &str,
        include_deleted: bool,
    ) -> Result<(Option<DeploymentResp>, Vec<Dependent>), anyhow::Error> {
        _get_deployment_and_dependents(
            self,
            crate::get_deployment_and_dependents_query(
                &self.project_id,
                &self.region,
                deployment_id,
                environment,
                include_deleted,
            ),
        )
        .await
    }
    async fn get_deployment(
        &self,
        deployment_id: &str,
        environment: &str,
        include_deleted: bool,
    ) -> Result<Option<DeploymentResp>, anyhow::Error> {
        _get_deployment(
            self,
            crate::get_deployment_query(
                &self.project_id,
                &self.region,
                deployment_id,
                environment,
                include_deleted,
            ),
        )
        .await
    }
    async fn get_deployments_using_module(
        &self,
        module: &str,
        environment: &str,
    ) -> Result<Vec<DeploymentResp>, anyhow::Error> {
        _get_deployments(
            self,
            crate::get_deployments_using_module_query(
                &self.project_id,
                &self.region,
                module,
                environment,
            ),
        )
        .await
    }
    async fn get_plan_deployment(
        &self,
        deployment_id: &str,
        environment: &str,
        job_id: &str,
    ) -> Result<Option<DeploymentResp>, anyhow::Error> {
        _get_deployment(
            self,
            crate::get_plan_deployment_query(
                &self.project_id,
                &self.region,
                deployment_id,
                environment,
                job_id,
            ),
        )
        .await
    }
    async fn get_dependents(
        &self,
        deployment_id: &str,
        environment: &str,
    ) -> Result<Vec<Dependent>, anyhow::Error> {
        _get_dependents(
            self,
            crate::get_dependents_query(&self.project_id, &self.region, deployment_id, environment),
        )
        .await
    }
    async fn get_deployments_to_driftcheck(&self) -> Result<Vec<DeploymentResp>, anyhow::Error> {
        _get_deployments(
            self,
            crate::get_deployments_to_driftcheck_query(&self.project_id, &self.region),
        )
        .await
    }
    async fn get_all_projects(&self) -> Result<Vec<ProjectData>, anyhow::Error> {
        get_projects(self, crate::get_all_projects_query()).await
    }
    async fn get_current_project(&self) -> Result<ProjectData, anyhow::Error> {
        get_projects(self, crate::get_current_project_query(&self.project_id))
            .await
            .map(|mut projects| projects.pop().expect("No project found"))
    }
    // Event
    async fn get_events(
        &self,
        deployment_id: &str,
        environment: &str,
    ) -> Result<Vec<EventData>, anyhow::Error> {
        _get_events(
            self,
            crate::get_events_query(&self.project_id, &self.region, deployment_id, environment),
        )
        .await
    }
    async fn get_all_events_between(
        &self,
        start_epoch: u128,
        end_epoch: u128,
    ) -> Result<Vec<EventData>, anyhow::Error> {
        _get_events(
            self,
            crate::get_all_events_between_query(&self.region, start_epoch, end_epoch),
        )
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
        _get_change_records(
            self,
            crate::get_change_records_query(
                &self.project_id,
                &self.region,
                environment,
                deployment_id,
                job_id,
                change_type,
            ),
        )
        .await
    }
    // Policy
    async fn get_newest_policy_version(
        &self,
        policy: &str,
        environment: &str,
    ) -> Result<PolicyResp, anyhow::Error> {
        _get_policy(
            self,
            crate::get_newest_policy_version_query(policy, environment),
        )
        .await
    }
    async fn get_all_policies(&self, environment: &str) -> Result<Vec<PolicyResp>, anyhow::Error> {
        _get_policies(self, crate::get_all_policies_query(environment)).await
    }
    async fn get_policy_download_url(&self, key: &str) -> Result<String, anyhow::Error> {
        match crate::run_function(
            &self.function_endpoint,
            &crate::get_generate_presigned_url_query(key, "policies"),
            &self.project_id,
            &self.region,
        )
        .await
        {
            Ok(response) => match response.payload.get("url") {
                Some(url) => Ok(url.as_str().unwrap().to_string()),
                None => Err(anyhow::anyhow!("Presigned url not found in response")),
            },
            Err(e) => Err(e),
        }
    }
    async fn get_policy(
        &self,
        policy: &str,
        environment: &str,
        version: &str,
    ) -> Result<PolicyResp, anyhow::Error> {
        _get_policy(self, crate::get_policy_query(policy, environment, version)).await
    }
}
