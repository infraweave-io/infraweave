use async_trait::async_trait;
use env_defs::{
    CloudHandlerError, CloudProvider, Dependent, DeploymentResp, EventData,
    GenericFunctionResponse, InfraChangeRecord, JobStatus, ModuleResp, PolicyResp, ProjectData,
    ProviderResp,
};
use env_utils::{
    _get_change_records, _get_dependents, _get_deployment, _get_deployment_and_dependents,
    _get_deployments, _get_events, _get_module_optional, _get_modules, _get_policies, _get_policy,
    _get_provider_optional, _get_providers, get_projects,
};
use serde_json::{json, Value};
use std::{future::Future, pin::Pin, thread::sleep, time::Duration};

#[derive(Clone)]
pub struct AwsCloudProvider {
    pub project_id: String,
    pub region: String,
    pub function_endpoint: Option<String>,
}

#[async_trait]
impl CloudProvider for AwsCloudProvider {
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
        "aws"
    }
    fn get_backend_provider(&self) -> &str {
        "s3"
    }
    fn get_storage_basepath(&self) -> String {
        format!("{}/", self.project_id) // Shared storage bucket with all projects
    }
    async fn get_backend_provider_arguments(
        &self,
        environment: &str,
        deployment_id: &str,
    ) -> serde_json::Value {
        let environment_variables = self.get_environment_variables().await.unwrap_or_default();
        json!({
            "bucket": environment_variables.get("TF_STATE_S3_BUCKET").unwrap(),
            "dynamodb_table": environment_variables.get("DYNAMODB_TF_LOCKS_TABLE_ARN").unwrap(),
            "key": format!("{}{}/{}/terraform.tfstate", self.get_storage_basepath(), environment, deployment_id),
            "region": self.region,
        })
    }
    async fn set_backend(
        &self,
        exec: &mut tokio::process::Command,
        deployment_id: &str,
        environment: &str,
    ) {
        crate::set_backend(
            exec,
            &self.get_storage_basepath(),
            deployment_id,
            environment,
        )
        .await;
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
    async fn run_function(
        &self,
        payload: &Value,
    ) -> Result<GenericFunctionResponse, anyhow::Error> {
        loop {
            // Todo move this loop to start_runner function
            match crate::run_function(
                &self.function_endpoint,
                payload,
                &self.project_id,
                &self.region,
            )
            .await
            {
                Ok(response) => return Ok(response),
                Err(e) => match e {
                    CloudHandlerError::NoAvailableRunner() => {
                        sleep(Duration::from_secs(1));
                        continue;
                    }
                    _ => {
                        eprintln!("Error: {:?}", e);
                        return Err(e.into());
                    }
                },
            }
        }
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
                    let items = response
                        .payload
                        .get("Items")
                        .expect("No Items field in response")
                        .as_array()
                        .unwrap()
                        .clone();
                    Ok(items)
                }
                Err(e) => Err(e.into()),
            }
        })
    }
    async fn get_latest_module_version(
        &self,
        module: &str,
        track: &str,
    ) -> Result<Option<ModuleResp>, anyhow::Error> {
        // Check if HTTP mode is enabled
        if crate::is_http_mode_enabled() {
            let modules = crate::http_get_all_latest_modules(track).await?;
            Ok(modules.into_iter().find(|m| m.module == module))
        } else {
            _get_module_optional(self, crate::get_latest_module_version_query(module, track)).await
        }
    }
    async fn get_latest_stack_version(
        &self,
        stack: &str,
        track: &str,
    ) -> Result<Option<ModuleResp>, anyhow::Error> {
        // Check if HTTP mode is enabled
        if crate::is_http_mode_enabled() {
            let stacks = crate::http_get_all_latest_stacks(track).await?;
            Ok(stacks.into_iter().find(|s| s.module == stack))
        } else {
            _get_module_optional(self, crate::get_latest_stack_version_query(stack, track)).await
        }
    }
    async fn get_job_status(&self, job_id: &str) -> Result<Option<JobStatus>, anyhow::Error> {
        match crate::run_function(
            &self.function_endpoint,
            &env_defs::get_job_status_event(job_id),
            &self.project_id,
            &self.region,
        )
        .await
        {
            Ok(response) => {
                let job_status: JobStatus = serde_json::from_value(response.payload)?;
                Ok(Some(job_status))
            }
            Err(e) => Err(e.into()),
        }
    }
    async fn get_latest_provider_version(
        &self,
        provider: &str,
    ) -> Result<Option<ProviderResp>, anyhow::Error> {
        // Check if HTTP mode is enabled
        if crate::is_http_mode_enabled() {
            let providers = crate::http_get_all_latest_providers().await?;
            Ok(providers.into_iter().find(|p| p.name == provider))
        } else {
            _get_provider_optional(self, crate::get_latest_provider_version_query(provider)).await
        }
    }
    async fn generate_presigned_url(
        &self,
        key: &str,
        bucket: &str,
    ) -> Result<String, anyhow::Error> {
        let event = env_defs::generate_presigned_url_event(key, bucket);
        let response = self.run_function(&event).await?;

        response.payload["url"]
            .as_str()
            .map(String::from)
            .ok_or_else(|| anyhow::anyhow!("URL not found in response"))
    }
    async fn upload_file_base64(
        &self,
        key: &str,
        bucket: &str,
        base64_content: &str,
    ) -> Result<(), anyhow::Error> {
        let event = env_defs::upload_file_base64_event(key, bucket, base64_content);
        self.run_function(&event).await?;
        Ok(())
    }
    async fn upload_file_url(
        &self,
        key: &str,
        bucket: &str,
        url: &str,
    ) -> Result<(), anyhow::Error> {
        let event = env_defs::upload_file_url_event(key, bucket, url);
        self.run_function(&event).await?;
        Ok(())
    }
    async fn transact_write(&self, items: &serde_json::Value) -> Result<(), anyhow::Error> {
        let event = env_defs::transact_write_event(items);
        self.run_function(&event).await?;
        Ok(())
    }
    async fn get_all_latest_module(&self, track: &str) -> Result<Vec<ModuleResp>, anyhow::Error> {
        // Check if HTTP mode is enabled
        if crate::is_http_mode_enabled() {
            return crate::http_get_all_latest_modules(track).await;
        }

        _get_modules(
            self,
            crate::get_all_latest_modules_query(track, false, false),
        )
        .await
    }
    async fn get_all_latest_stack(&self, track: &str) -> Result<Vec<ModuleResp>, anyhow::Error> {
        // Check if HTTP mode is enabled
        if crate::is_http_mode_enabled() {
            return crate::http_get_all_latest_stacks(track).await;
        }

        _get_modules(
            self,
            crate::get_all_latest_stacks_query(track, false, false),
        )
        .await
    }
    async fn get_all_latest_provider(&self) -> Result<Vec<ProviderResp>, anyhow::Error> {
        // Check if HTTP mode is enabled
        if crate::is_http_mode_enabled() {
            return crate::http_get_all_latest_providers().await;
        }

        _get_providers(self, crate::get_all_latest_providers_query()).await
    }
    async fn get_all_module_versions(
        &self,
        module: &str,
        track: &str,
    ) -> Result<Vec<ModuleResp>, anyhow::Error> {
        if crate::http_client::is_http_mode_enabled() {
            return crate::http_get_all_versions_for_module(track, module).await;
        }
        _get_modules(
            self,
            crate::get_all_module_versions_query(module, track, false, false),
        )
        .await
    }
    async fn get_all_stack_versions(
        &self,
        stack: &str,
        track: &str,
    ) -> Result<Vec<ModuleResp>, anyhow::Error> {
        if crate::http_client::is_http_mode_enabled() {
            return crate::http_get_all_versions_for_stack(track, stack).await;
        }
        _get_modules(
            self,
            crate::get_all_stack_versions_query(stack, track, false, false),
        )
        .await
    }
    async fn get_module_version(
        &self,
        module: &str,
        track: &str,
        version: &str,
    ) -> Result<Option<ModuleResp>, anyhow::Error> {
        if crate::http_client::is_http_mode_enabled() {
            return crate::http_get_module_version(track, module, version)
                .await
                .map(Some);
        }
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
        if crate::http_client::is_http_mode_enabled() {
            return crate::http_get_stack_version(track, stack, version)
                .await
                .map(Some);
        }
        _get_module_optional(self, crate::get_stack_version_query(stack, track, version)).await
    }
    // Deployment
    async fn get_all_deployments(
        &self,
        environment: &str,
        include_deleted: bool,
    ) -> Result<Vec<DeploymentResp>, anyhow::Error> {
        // Check if HTTP mode is enabled
        if crate::http_client::is_http_mode_enabled() {
            let deployments = crate::http_get_deployments(&self.project_id, &self.region).await?;
            // Parse JSON values into DeploymentResp structs
            let parsed: Vec<DeploymentResp> = deployments
                .into_iter()
                .map(|v| serde_json::from_value(v))
                .collect::<Result<Vec<_>, _>>()?;

            // Filter by environment and include_deleted if needed
            let filtered: Vec<DeploymentResp> = parsed
                .into_iter()
                .filter(|d| {
                    let env_match = environment.is_empty() || d.environment == environment;
                    let deleted_match = include_deleted || !d.deleted;
                    env_match && deleted_match
                })
                .collect();

            return Ok(filtered);
        }

        _get_deployments(
            self,
            crate::get_all_deployments_query(
                &self.project_id,
                &self.region,
                environment,
                include_deleted,
            ),
        )
        .await
    }
    async fn get_deployment_and_dependents(
        &self,
        deployment_id: &str,
        environment: &str,
        include_deleted: bool,
    ) -> Result<(Option<DeploymentResp>, Vec<Dependent>), anyhow::Error> {
        // Check if HTTP mode is enabled
        if crate::http_client::is_http_mode_enabled() {
            let deployment_value = crate::http_describe_deployment(
                &self.project_id,
                &self.region,
                environment,
                deployment_id,
            )
            .await?;

            // Parse the deployment from JSON
            let deployment: Option<DeploymentResp> = if deployment_value.is_null() {
                None
            } else {
                Some(serde_json::from_value(deployment_value)?)
            };

            // HTTP API doesn't return dependents in describe endpoint currently
            // Return empty vec for dependents
            return Ok((deployment, vec![]));
        }

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
        // Check if HTTP mode is enabled
        if crate::http_client::is_http_mode_enabled() {
            let deployment_value = crate::http_describe_deployment(
                &self.project_id,
                &self.region,
                environment,
                deployment_id,
            )
            .await?;

            if deployment_value.is_null() {
                return Ok(None);
            }

            let deployment: DeploymentResp = serde_json::from_value(deployment_value)?;
            return Ok(Some(deployment));
        }

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
        include_deleted: bool,
    ) -> Result<Vec<DeploymentResp>, anyhow::Error> {
        _get_deployments(
            self,
            crate::get_deployments_using_module_query(
                &self.project_id,
                &self.region,
                module,
                environment,
                include_deleted,
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
        if crate::http_client::is_http_mode_enabled() {
            // Use HTTP API to get specific plan deployment
            match crate::http_client::http_get_plan_deployment(
                &self.project_id,
                &self.region,
                environment,
                deployment_id,
                job_id,
            )
            .await
            {
                Ok(deployment_value) => {
                    let deployment: DeploymentResp = serde_json::from_value(deployment_value)?;
                    Ok(Some(deployment))
                }
                Err(e) => {
                    // If 404/not found error
                    if e.to_string().contains("not found") || e.to_string().contains("404") {
                        Ok(None)
                    } else {
                        Err(e)
                    }
                }
            }
        } else {
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
        // Check if HTTP mode is enabled
        if crate::http_client::is_http_mode_enabled() {
            let projects = crate::http_get_all_projects().await?;
            // Parse JSON values into ProjectData structs
            let parsed: Vec<ProjectData> = projects
                .into_iter()
                .map(|v| serde_json::from_value(v))
                .collect::<Result<Vec<_>, _>>()?;
            return Ok(parsed);
        }

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
        if crate::is_http_mode_enabled() {
            let events = crate::http_client::http_get_events(
                &self.project_id,
                &self.region,
                environment,
                deployment_id,
            )
            .await?;

            // Convert Value to EventData
            let event_data: Vec<EventData> = events
                .into_iter()
                .map(|v| serde_json::from_value(v).map_err(|e| anyhow::anyhow!(e)))
                .collect::<Result<Vec<_>, _>>()?;

            Ok(event_data)
        } else {
            _get_events(
                self,
                crate::get_events_query(
                    &self.project_id,
                    &self.region,
                    deployment_id,
                    environment,
                    None,
                ),
            )
            .await
        }
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
        if crate::http_client::is_http_mode_enabled() {
            let change_record_value = crate::http_client::http_get_change_record(
                &self.project_id,
                &self.region,
                environment,
                deployment_id,
                job_id,
                change_type,
            )
            .await?;

            let change_record: InfraChangeRecord = serde_json::from_value(change_record_value)?;
            return Ok(change_record);
        }

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
            &env_defs::generate_presigned_url_event(key, "policies"),
            &self.project_id,
            &self.region,
        )
        .await
        {
            Ok(response) => match response.payload.get("url") {
                Some(url) => Ok(url.as_str().unwrap().to_string()),
                None => Err(anyhow::anyhow!("Presigned url not found in response")),
            },
            Err(e) => Err(e.into()),
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
    async fn get_environment_variables(&self) -> Result<serde_json::Value, anyhow::Error> {
        match crate::run_function(
            &self.function_endpoint,
            &env_defs::get_environment_variables_event(),
            &self.project_id,
            &self.region,
        )
        .await
        {
            Ok(response) => match response.payload.get("body") {
                Some(variables) => Ok(variables.clone()),
                None => Err(anyhow::anyhow!(
                    "Environment variables not found in response"
                )),
            },
            Err(e) => {
                println!("Error getting environment variables: {:?}", e);
                Err(anyhow::anyhow!(
                    "Failed to get function environment variables"
                ))
            }
        }
    }

    async fn download_state_file(
        &self,
        environment: &str,
        deployment_id: &str,
        output: Option<String>,
    ) -> Result<(), anyhow::Error> {
        let backend_args = self
            .get_backend_provider_arguments(environment, deployment_id)
            .await;

        let bucket = backend_args
            .get("bucket")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("bucket not found in backend args"))?;
        let key = backend_args
            .get("key")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("key not found in backend args"))?;
        let region = backend_args
            .get("region")
            .and_then(|v| v.as_str())
            .unwrap_or(&self.region);

        let config = aws_config::from_env()
            .region(aws_config::Region::new(region.to_string()))
            .load()
            .await;
        let client = aws_sdk_s3::Client::new(&config);

        let resp = client.get_object().bucket(bucket).key(key).send().await?;
        let data = resp.body.collect().await?.into_bytes();

        if let Some(output_path) = output {
            std::fs::write(output_path, &data)?;
        } else {
            let state_str = String::from_utf8_lossy(&data);
            println!("{}", state_str);
        }

        Ok(())
    }
}
