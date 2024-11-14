use core::panic;
use std::{thread::sleep, time::Duration};

use async_trait::async_trait;
use env_aws::get_user_id;
use env_defs::{
    Dependent, DeploymentResp, EventData, GenericFunctionResponse, InfraChangeRecord, LogData, ModuleResp, PolicyResp, ProjectData
};
use serde_json::Value;

use crate::logic::{insert_event, insert_infra_change_record, publish_policy, read_logs, set_deployment, set_project};

#[async_trait]
pub trait CloudHandler {
    fn get_project_id(&self) -> &str;
    async fn get_user_id(&self) -> Result<String, anyhow::Error>;
    fn get_region(&self) -> &str;
    fn get_cloud_provider(&self) -> &str;
    // Function
    async fn run_function(&self, payload: &Value) -> Result<GenericFunctionResponse, anyhow::Error>;
    // Module + stack
    async fn get_latest_module_version(&self, module: &str, track: &str,) -> Result<Option<ModuleResp>, anyhow::Error>;
    async fn get_latest_stack_version(&self, stack: &str, track: &str,) -> Result<Option<ModuleResp>, anyhow::Error>;
    async fn generate_presigned_url(&self, key: &str) -> Result<String, anyhow::Error>;
    async fn get_all_latest_module(&self, track: &str) -> Result<Vec<ModuleResp>, anyhow::Error>;
    async fn get_all_latest_stack(&self, track: &str) -> Result<Vec<ModuleResp>, anyhow::Error>;
    async fn get_all_module_versions(&self, module: &str, track: &str) -> Result<Vec<ModuleResp>, anyhow::Error>;
    async fn get_all_stack_versions(&self, stack: &str, track: &str) -> Result<Vec<ModuleResp>, anyhow::Error>;
    async fn get_module_version(&self, module: &str, track: &str, version: &str) -> Result<Option<ModuleResp>, anyhow::Error>;
    async fn get_stack_version(&self, module: &str, track: &str, version: &str) -> Result<Option<ModuleResp>, anyhow::Error>;
    // Deployment
    async fn get_all_deployments(&self, environment: &str) -> Result<Vec<DeploymentResp>, anyhow::Error>;
    async fn get_deployment_and_dependents(&self, deployment_id: &str, environment: &str, include_deleted: bool) -> Result<(Option<DeploymentResp>, Vec<Dependent>), anyhow::Error>;
    async fn get_deployment(&self, deployment_id: &str, environment: &str, include_deleted: bool) -> Result<Option<DeploymentResp>, anyhow::Error>;
    async fn get_deployments_using_module(&self, module: &str, environment: &str) -> Result<Vec<DeploymentResp>, anyhow::Error>;
    async fn get_plan_deployment(&self, deployment_id: &str, environment: &str, job_id: &str) -> Result<Option<DeploymentResp>, anyhow::Error>;
    async fn get_dependents(&self, deployment_id: &str, environment: &str) -> Result<Vec<Dependent>, anyhow::Error>;
    async fn set_deployment(&self, deployment: &DeploymentResp, is_plan: bool) -> Result<(), anyhow::Error>;
    async fn get_deployments_to_driftcheck(&self) -> Result<Vec<DeploymentResp>, anyhow::Error>;
    async fn set_project(&self, project: &ProjectData) -> Result<(), anyhow::Error>;
    async fn get_all_projects(&self) -> Result<Vec<ProjectData>, anyhow::Error>;
    async fn get_current_project(&self) -> Result<ProjectData, anyhow::Error>;
    // Event
    async fn insert_event(&self, event: EventData) -> Result<String, anyhow::Error>;
    async fn get_events(&self, deployment_id: &str, environment: &str) -> Result<Vec<EventData>, anyhow::Error>;
    async fn get_all_events_between(&self, start_epoch: u128, end_epoch: u128) -> Result<Vec<EventData>, anyhow::Error>;
    // Change record
    async fn get_change_record(&self, environment: &str, deployment_id: &str, job_id: &str, change_type: &str) -> Result<InfraChangeRecord, anyhow::Error>;
    async fn insert_infra_change_record(&self, infra_change_record: InfraChangeRecord, plan_output_raw: &str) -> Result<String, anyhow::Error>;
    // Log
    async fn read_logs(&self, job_id: &str) -> Result<Vec<LogData>, anyhow::Error>;
    // Policy
    async fn publish_policy(&self, manifest_path: &str, environment: &str) -> Result<(), anyhow::Error>;
    async fn get_newest_policy_version(&self, policy: &str, environment: &str) -> Result<PolicyResp, anyhow::Error>;
    async fn get_all_policies(&self, environment: &str) -> Result<Vec<PolicyResp>, anyhow::Error>;
    async fn get_policy_download_url(&self, key: &str) -> Result<String, anyhow::Error>;
    async fn get_policy(&self, policy: &str, environment: &str, version: &str) -> Result<PolicyResp, anyhow::Error>;
}

pub struct AwsCloudHandler {
    pub project_id: String,
    pub region: String,
}

pub struct AzureCloudHandler {
    pub project_id: String,
    pub region: String,
}

impl AwsCloudHandler {
    pub fn new(project_id: String, region: String) -> Self {
        AwsCloudHandler { project_id, region }
    }
}

impl AzureCloudHandler {
    pub fn new(project_id: String, region: String) -> Self {
        AzureCloudHandler { project_id, region }
    }
}

#[async_trait]
impl CloudHandler for AwsCloudHandler {
    fn get_project_id(&self) -> &str {
        &self.project_id
    }
    async fn get_user_id(&self) -> Result<String, anyhow::Error> {
        get_user_id().await
    }
    fn get_region(&self) -> &str {
        &self.region
    }
    fn get_cloud_provider(&self) -> &str {
        "aws"
    }
    async fn run_function(&self, payload: &Value) -> Result<GenericFunctionResponse, anyhow::Error> {
        loop {
            match env_aws::run_function(payload).await {
                Ok(response) => {
                    if response.payload["errorType"] == "IndexError" {
                        // No available instances found, retry after a delay
                        sleep(Duration::from_secs(1));
                    } else {
                        return Ok(response);
                    }
                },
                Err(e) => {
                    return Err(e);
                }
            }
        }
    }
    async fn get_latest_module_version(&self, module: &str, track: &str) -> Result<Option<ModuleResp>, anyhow::Error> {
        get_module_optional(env_aws::get_latest_module_version_query(module, track)).await
    } 
    async fn get_latest_stack_version(&self, stack: &str, track: &str) -> Result<Option<ModuleResp>, anyhow::Error> {
        get_module_optional(env_aws::get_latest_stack_version_query(stack, track)).await
    }
    async fn generate_presigned_url(&self, key: &str) -> Result<String, anyhow::Error> {
        match env_aws::run_function(&env_aws::get_generate_presigned_url_query(key, "modules")).await {
            Ok(response) => {
                match response.payload.get("url") {
                    Some(url) => Ok(url.as_str().unwrap().to_string()),
                    None => Err(anyhow::anyhow!("Presigned url not found in response")),
                }
            },
            Err(e) => Err(e),
        }
    }
    async fn get_all_latest_module(&self, track: &str) -> Result<Vec<ModuleResp>, anyhow::Error> {
        get_modules(env_aws::get_all_latest_modules_query(track)).await
    }
    async fn get_all_latest_stack(&self, track: &str) -> Result<Vec<ModuleResp>, anyhow::Error> {
        get_modules(env_aws::get_all_latest_stacks_query(track)).await
    }
    async fn get_all_module_versions(&self, module: &str, track: &str) -> Result<Vec<ModuleResp>, anyhow::Error> {
        get_modules(env_aws::get_all_module_versions_query(module, track)).await
    }
    async fn get_all_stack_versions(&self, stack: &str, track: &str) -> Result<Vec<ModuleResp>, anyhow::Error> {
        get_modules(env_aws::get_all_stack_versions_query(stack, track)).await
    }
    async fn get_module_version(&self, module: &str, track: &str, version: &str) -> Result<Option<ModuleResp>, anyhow::Error> {
        get_module_optional(env_aws::get_module_version_query(module, track, version)).await
    }
    async fn get_stack_version(&self, stack: &str, track: &str, version: &str) -> Result<Option<ModuleResp>, anyhow::Error> {
        get_module_optional(env_aws::get_stack_version_query(stack, track, version)).await
    }
    // Deployment
    async fn get_all_deployments(&self, environment: &str) -> Result<Vec<DeploymentResp>, anyhow::Error> {
        _get_deployments(env_aws::get_all_deployments_query(&self.project_id, &self.region, environment)).await
    }
    async fn get_deployment_and_dependents(&self, deployment_id: &str, environment: &str, include_deleted: bool) -> Result<(Option<DeploymentResp>, Vec<Dependent>), anyhow::Error> {
        _get_deployment_and_dependents(env_aws::get_deployment_and_dependents_query(&self.project_id, &self.region, deployment_id, environment, include_deleted)).await
    }
    async fn get_deployment(&self, deployment_id: &str, environment: &str, include_deleted: bool) -> Result<Option<DeploymentResp>, anyhow::Error> {
        _get_deployment(env_aws::get_deployment_query(&self.project_id, &self.region, deployment_id, environment, include_deleted)).await
    }
    async fn get_deployments_using_module(&self, module: &str, environment: &str) -> Result<Vec<DeploymentResp>, anyhow::Error> {
        _get_deployments(env_aws::get_deployments_using_module_query(&self.project_id, &self.region, module, &environment)).await
    }
    async fn get_plan_deployment(&self, deployment_id: &str, environment: &str, job_id: &str) -> Result<Option<DeploymentResp>, anyhow::Error> {
        _get_deployment(env_aws::get_plan_deployment_query(&self.project_id, &self.region, deployment_id, environment, job_id)).await
    }
    async fn get_dependents(&self, deployment_id: &str, environment: &str) -> Result<Vec<Dependent>, anyhow::Error> {
        _get_dependents(env_aws::get_dependents_query(&self.project_id, &self.region, deployment_id, environment)).await
    }
    async fn set_deployment(&self, deployment: &DeploymentResp, is_plan: bool) -> Result<(), anyhow::Error> {
        set_deployment(deployment, is_plan).await
    }
    async fn get_deployments_to_driftcheck(&self) -> Result<Vec<DeploymentResp>, anyhow::Error> {
        _get_deployments(env_aws::get_deployments_to_driftcheck_query(&self.project_id, &self.region)).await
    }
    async fn set_project(&self, project: &ProjectData) -> Result<(), anyhow::Error> {
        set_project(project).await
    }
    async fn get_all_projects(&self) -> Result<Vec<ProjectData>, anyhow::Error> {
        get_projects(env_aws::get_all_projects_query()).await
    }
    async fn get_current_project(&self) -> Result<ProjectData, anyhow::Error> {
        get_projects(env_aws::get_current_project_query(&self.project_id)).await.map(|mut projects| projects.pop().expect("No project found"))
    }
    // Event
    async fn insert_event(&self, event: EventData) -> Result<String, anyhow::Error> {
        insert_event(event).await
    }
    async fn get_events(&self, deployment_id: &str, environment: &str) -> Result<Vec<EventData>, anyhow::Error> {
        _get_events(env_aws::get_events_query(&self.project_id, &self.region, deployment_id, environment)).await
    }
    async fn get_all_events_between(&self, start_epoch: u128, end_epoch: u128) -> Result<Vec<EventData>, anyhow::Error> {
        _get_events(env_aws::get_all_events_between_query(&self.region, start_epoch, end_epoch)).await
    }
    // Change record
    async fn get_change_record(&self, environment: &str, deployment_id: &str, job_id: &str, change_type: &str) -> Result<InfraChangeRecord, anyhow::Error> {
        _get_change_records(env_aws::get_change_records_query(&self.project_id, &self.region, environment, deployment_id, job_id, change_type)).await
    }
    async fn insert_infra_change_record(&self, infra_change_record: InfraChangeRecord, plan_output_raw: &str) -> Result<String, anyhow::Error> {
        insert_infra_change_record(infra_change_record, plan_output_raw).await
    }
    // Log
    async fn read_logs(&self, job_id: &str) -> Result<Vec<LogData>, anyhow::Error> {
        read_logs(&self.project_id, job_id).await
    }
    // Policy
    async fn publish_policy(&self, manifest_path: &str, environment: &str) -> Result<(), anyhow::Error> {
        publish_policy(manifest_path, environment).await
    }
    async fn get_newest_policy_version(&self, policy: &str, environment: &str) -> Result<PolicyResp, anyhow::Error> {
        _get_policy(env_aws::get_newest_policy_version_query(policy, environment)).await
    }
    async fn get_all_policies(&self, environment: &str) -> Result<Vec<PolicyResp>, anyhow::Error> {
        _get_policies(env_aws::get_all_policies_query(environment)).await
    }
    async fn get_policy_download_url(&self, key: &str) -> Result<String, anyhow::Error> {
        match env_aws::run_function(&env_aws::get_generate_presigned_url_query(key, "policies")).await {
            Ok(response) => {
                match response.payload.get("url") {
                    Some(url) => Ok(url.as_str().unwrap().to_string()),
                    None => Err(anyhow::anyhow!("Presigned url not found in response")),
                }
            },
            Err(e) => Err(e),
        }
    }
    async fn get_policy(&self, policy: &str, environment: &str, version: &str) -> Result<PolicyResp, anyhow::Error> {
        _get_policy(env_aws::get_policy_query(policy, environment, version)).await
    }
}

#[async_trait]
impl CloudHandler for AzureCloudHandler {
    fn get_project_id(&self) -> &str {
        &self.project_id
    }
    async fn get_user_id(&self) -> Result<String, anyhow::Error> {
        panic!("Not implemented for Azure");
    }
    fn get_region(&self) -> &str {
        &self.region
    }
    fn get_cloud_provider(&self) -> &str {
        "azure"
    }
    async fn run_function(&self, items: &Value) -> Result<GenericFunctionResponse, anyhow::Error> {
        panic!("Not implemented for Azure");
    }
    async fn get_latest_module_version(&self, module: &str, track: &str) -> Result<Option<ModuleResp>, anyhow::Error> {
        panic!("Not implemented for Azure");
    }
    async fn get_latest_stack_version(&self, stack: &str, track: &str) -> Result<Option<ModuleResp>, anyhow::Error> {
        panic!("Not implemented for Azure");
    }
    async fn generate_presigned_url(&self, key: &str) -> Result<String, anyhow::Error> {
        panic!("Not implemented for Azure");
    }
    async fn get_all_latest_module(&self, track: &str) -> Result<Vec<ModuleResp>, anyhow::Error> {
        panic!("Not implemented for Azure");
    }
    async fn get_all_latest_stack(&self, track: &str) -> Result<Vec<ModuleResp>, anyhow::Error> {
        panic!("Not implemented for Azure");
    }
    async fn get_all_stack_versions(&self, stack: &str, track: &str) -> Result<Vec<ModuleResp>, anyhow::Error> {
        panic!("Not implemented for Azure");
    }
    async fn get_all_module_versions(&self, module: &str, track: &str) -> Result<Vec<ModuleResp>, anyhow::Error> {
        panic!("Not implemented for Azure");
    }
    async fn get_module_version(&self, module: &str, track: &str, version: &str) -> Result<Option<ModuleResp>, anyhow::Error> {
        panic!("Not implemented for Azure");
    }
    async fn get_stack_version(&self, stack: &str, track: &str, version: &str) -> Result<Option<ModuleResp>, anyhow::Error> {
        panic!("Not implemented for Azure");
    }
    // Deployment
    async fn get_all_deployments(&self, environment: &str) -> Result<Vec<DeploymentResp>, anyhow::Error> {
        panic!("Not implemented for Azure");
    }
    async fn get_deployment_and_dependents(&self, deployment_id: &str, environment: &str, include_deleted: bool) -> Result<(Option<DeploymentResp>, Vec<Dependent>), anyhow::Error> {
        panic!("Not implemented for Azure");
    }
    async fn get_deployment(&self, deployment_id: &str, environment: &str, include_deleted: bool) -> Result<Option<DeploymentResp>, anyhow::Error> {
        panic!("Not implemented for Azure");
    }
    async fn get_deployments_using_module(&self, module: &str, environment: &str) -> Result<Vec<DeploymentResp>, anyhow::Error> {
        panic!("Not implemented for Azure");
    }
    async fn get_plan_deployment(&self, deployment_id: &str, environment: &str, job_id: &str) -> Result<Option<DeploymentResp>, anyhow::Error> {
        panic!("Not implemented for Azure");
    }
    async fn get_dependents(&self, deployment_id: &str, environment: &str) -> Result<Vec<Dependent>, anyhow::Error> {
        panic!("Not implemented for Azure");
    }
    async fn set_deployment(&self, deployment: &DeploymentResp, is_plan: bool) -> Result<(), anyhow::Error> {
        panic!("Not implemented for Azure");
    }
    async fn get_deployments_to_driftcheck(&self) -> Result<Vec<DeploymentResp>, anyhow::Error> {
        panic!("Not implemented for Azure");
    }
    async fn set_project(&self, project: &ProjectData) -> Result<(), anyhow::Error> {
        panic!("Not implemented for Azure");
    }
    async fn get_all_projects(&self) -> Result<Vec<ProjectData>, anyhow::Error> {
        panic!("Not implemented for Azure");
    }
    async fn get_current_project(&self) -> Result<ProjectData, anyhow::Error> {
        panic!("Not implemented for Azure");
    }
    // Event
    async fn insert_event(&self, event: EventData) -> Result<String, anyhow::Error> {
        panic!("Not implemented for Azure");
    }
    async fn get_events(&self, deployment_id: &str, environment: &str) -> Result<Vec<EventData>, anyhow::Error> {
        panic!("Not implemented for Azure");
    }
    async fn get_all_events_between(&self, start_epoch: u128, end_epoch: u128) -> Result<Vec<EventData>, anyhow::Error> {
        panic!("Not implemented for Azure");
    }
    // Change record
    async fn get_change_record(&self, environment: &str, deployment_id: &str, job_id: &str, change_type: &str) -> Result<InfraChangeRecord, anyhow::Error> {
        panic!("Not implemented for Azure");
    }
    async fn insert_infra_change_record(&self, infra_change_record: InfraChangeRecord, plan_output_raw: &str) -> Result<String, anyhow::Error> {
        panic!("Not implemented for Azure");
    }
    // Log
    async fn read_logs(&self, job_id: &str) -> Result<Vec<LogData>, anyhow::Error> {
        panic!("Not implemented for Azure");
    }
    // Policy
    async fn publish_policy(&self, manifest_path: &str, environment: &str) -> Result<(), anyhow::Error> {
        panic!("Not implemented for Azure");
    }
    async fn get_newest_policy_version(&self, policy: &str, environment: &str) -> Result<PolicyResp, anyhow::Error> {
        panic!("Not implemented for Azure");
    }
    async fn get_all_policies(&self, environment: &str) -> Result<Vec<PolicyResp>, anyhow::Error> {
        panic!("Not implemented for Azure");
    }
    async fn get_policy_download_url(&self, key: &str) -> Result<String, anyhow::Error> {
        panic!("Not implemented for Azure");
    }
    async fn get_policy(&self, policy: &str, environment: &str, version: &str) -> Result<PolicyResp, anyhow::Error> {
        panic!("Not implemented for Azure");
    }
}


// AWS Helper functions

async fn get_projects(query: Value) -> Result<Vec<ProjectData>, anyhow::Error> {
    match env_aws::read_db("deployments", &query).await {
        Ok(response) if !response.payload.get("Items").unwrap().as_array().unwrap().is_empty() => {
            let items = response.payload.get("Items").expect("No Items field in response").clone();
            let mut projects_vec: Vec<ProjectData> = vec![];
            for project in items.as_array().unwrap() {
                let projectdata: ProjectData = serde_json::from_value(project.clone()).expect(format!("Failed to parse project {}", project).as_str());
                projects_vec.push(projectdata);
            }
            return Ok(projects_vec);
        },
        Ok(_) => Ok(vec![]), // No projects were found
        Err(e) => Err(e),
    }
}

async fn get_modules(query: Value) -> Result<Vec<ModuleResp>, anyhow::Error> {
    match env_aws::read_db("modules", &query).await {
        Ok(response) if !response.payload.get("Items").unwrap().as_array().unwrap().is_empty() => {
            serde_json::from_value(response.payload.get("Items").unwrap().clone()).map_err(|e| anyhow::anyhow!("Failed to modules: {}\nResponse: {}", e.to_string(), response.payload))
        },
        Ok(_) => Ok(vec![]), // No modules were found
        Err(e) => Err(e),
    }
}

async fn get_module_optional(query: Value) -> Result<Option<ModuleResp>, anyhow::Error> {
    match get_modules(query).await {
        Ok(mut modules) => {
            if modules.is_empty() {
                Ok(None)
            } else {
                Ok(modules.pop())
            }
        },
        Err(e) => Err(e),
    }
}

async fn _get_deployments(query: Value) -> Result<Vec<DeploymentResp>, anyhow::Error> {
    match env_aws::read_db("deployments", &query).await {
        Ok(response) if !response.payload.get("Items").unwrap().as_array().unwrap().is_empty() => {
            let mut items = response.payload.get("Items").expect("No Items field in response").clone();
            _mutate_deployment(&mut items);
            serde_json::from_value(items).map_err(|e| anyhow::anyhow!("Failed to deployments: {}\nResponse: {}", e.to_string(), response.payload))
        },
        Ok(_) => Ok(vec![]), // No deployments were found
        Err(e) => Err(e),
    }
}

fn _mutate_deployment(value: &mut Value) {
    for v in value.as_array_mut().unwrap() { // Value is an array, loop through every element and modify the deleted field
        v["deleted"] = serde_json::json!(v["deleted"].as_f64().unwrap() != 0.0); // Boolean is not supported in GSI, so convert it to/from int for AWS
    }
}

async fn _get_deployment_and_dependents(query: Value) -> Result<(Option<DeploymentResp>, Vec<Dependent>), anyhow::Error> {
    match env_aws::read_db("deployments", &query).await {
        Ok(response) if !response.payload.get("Items").unwrap().as_array().unwrap().is_empty() => {
            let mut items = response.payload.get("Items").expect("No Items field in response").clone();
            if let Some(elements) = items.as_array() {
                let mut deployments_vec: Vec<DeploymentResp> = vec![];
                let mut dependents_vec: Vec<Dependent> = vec![];
                for e in elements {
                    if e.get("SK").unwrap().as_str().unwrap().starts_with("DEPENDENT#") {
                        let dependent: Dependent = serde_json::from_value(e.clone()).expect("Failed to parse dependent");
                        dependents_vec.push(dependent);
                    } else {
                        let mut value = e.clone();
                        value["deleted"] = serde_json::json!(value["deleted"].as_f64().unwrap() != 0.0); // Boolean is not supported in GSI, so convert it to/from int for AWS
                        let deployment: DeploymentResp = serde_json::from_value(value).expect("Failed to parse deployment");
                        deployments_vec.push(deployment);
                    }
                }
                if deployments_vec.len() == 0 {
                    println!("No deployment was found");
                    return Ok((None, dependents_vec));
                }
                return Ok((Some(deployments_vec[0].clone()), dependents_vec));
            } else {
                panic!("Expected an array of deployments");
            }
        },
        Ok(_) => Ok((None, vec![])), // No deployments were found
        Err(e) => Err(e),
    }
}

async fn _get_deployment(query: Value) -> Result<Option<DeploymentResp>, anyhow::Error> {
    match _get_deployment_and_dependents(query).await {
        Ok((deployment, _)) => Ok(deployment),
        Err(e) => Err(e),
    }
}

async fn _get_dependents(query: Value) -> Result<Vec<Dependent>, anyhow::Error> {
    match _get_deployment_and_dependents(query).await {
        Ok((_, dependents)) => Ok(dependents),
        Err(e) => Err(e),
    }
}

async fn _get_events(query: Value) -> Result<Vec<EventData>, anyhow::Error> {
    match env_aws::read_db("events", &query).await {
        Ok(response) if !response.payload.get("Items").unwrap().as_array().unwrap().is_empty() => {
            let items = response.payload.get("Items").expect("No Items field in response").clone();
            let mut events_vec: Vec<EventData> = vec![];
            for event in items.as_array().unwrap() {
                let eventdata: EventData = serde_json::from_value(event.clone()).expect(format!("Failed to parse event {}", event).as_str());
                events_vec.push(eventdata);
            }
            return Ok(events_vec);
        },
        Ok(_) => Ok(vec![]), // No events were found
        Err(e) => Err(e),
    }
}

async fn _get_change_records(query: Value) -> Result<InfraChangeRecord, anyhow::Error> {
    match env_aws::read_db("change_records", &query).await {
        Ok(response) if !response.payload.get("Items").unwrap().as_array().unwrap().is_empty() => {
            let items = response.payload.get("Items").expect("No Items field in response").clone();
            if let Some(change_records) = items.as_array() {
                if change_records.len() == 1 {
                    let change_record: InfraChangeRecord =
                        serde_json::from_value(change_records[0].clone()).expect("Failed to parse change record");
                    return Ok(change_record);
                } else if change_records.len() == 0 {
                    return Err(anyhow::anyhow!("No change record found"));
                } else {
                    panic!("Expected exactly one change record");
                }
            } else {
                panic!("Expected an array of change records");
            }
        },
        Ok(_) => Err(anyhow::anyhow!("No change record found")),
        Err(e) => Err(e),
    }
}

async fn _get_policy(query: Value) -> Result<PolicyResp, anyhow::Error> {
    match env_aws::read_db("policies", &query).await {
        Ok(response) if !response.payload.get("Items").unwrap().as_array().unwrap().is_empty() => {
            let items = response.payload.get("Items").expect("No Items field in response").clone();
            if let Some(policies) = items.as_array() {
                if policies.len() == 1 {
                    let policy: PolicyResp =
                        serde_json::from_value(policies[0].clone()).expect("Failed to parse policy");
                    return Ok(policy);
                } else if policies.len() == 0 {
                    return Err(anyhow::anyhow!("No policy found"));
                } else {
                    panic!("Expected exactly one policy");
                }
            } else {
                panic!("Expected an array of policies");
            }
        },
        Ok(_) => Err(anyhow::anyhow!("No policy found")),
        Err(e) => Err(e),
    }
}

async fn _get_policies(query: Value) -> Result<Vec<PolicyResp>, anyhow::Error> {
    match env_aws::read_db("policies", &query).await {
        Ok(response) if !response.payload.get("Items").unwrap().as_array().unwrap().is_empty() => {
            let items = response.payload.get("Items").expect("No Items field in response").clone();
            let mut policies_vec: Vec<PolicyResp> = vec![];
            for policy in items.as_array().unwrap() {
                let policydata: PolicyResp = serde_json::from_value(policy.clone()).expect(format!("Failed to parse policy {}", policy).as_str());
                policies_vec.push(policydata);
            }
            return Ok(policies_vec);
        },
        Ok(_) => Ok(vec![]), // No policies were found
        Err(e) => Err(e),
    }
}

pub async fn initialize_project_id_and_region() -> String {
    if crate::logic::PROJECT_ID.get().is_none() {
        let account_id = env_aws::get_project_id().await.unwrap();
        println!("Account ID: {}", &account_id);
        crate::logic::PROJECT_ID.set(account_id.clone()).expect("Failed to set PROJECT_ID");
    }
    if crate::logic::REGION.get().is_none() {
        let region = env_aws::get_region().await;
        println!("Region: {}", &region);
        crate::logic::REGION.set(region).expect("Failed to set REGION");
    }
    crate::logic::PROJECT_ID.get().unwrap().clone()
}