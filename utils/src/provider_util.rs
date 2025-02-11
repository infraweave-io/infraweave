// Helper functions

use std::{future::Future, pin::Pin};

use env_defs::{
    Dependent, DeploymentResp, EventData, InfraChangeRecord, ModuleResp, PolicyResp, ProjectData,
};
use serde_json::Value;

type ReadDbGenericFn =
    fn(
        &Option<String>,
        &str,
        &Value,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<Value>, anyhow::Error>> + Send>>; // Raw responses slightly differ between AWS and Azure

pub async fn get_projects(
    function_endpoint: &Option<String>,
    query: Value,
    read_db: ReadDbGenericFn,
) -> Result<Vec<ProjectData>, anyhow::Error> {
    match read_db(function_endpoint, "deployments", &query).await {
        Ok(items) => {
            let mut projects_vec: Vec<ProjectData> = vec![];
            for project in items {
                let projectdata: ProjectData = serde_json::from_value(project.clone())
                    .unwrap_or_else(|_| panic!("Failed to parse project {}", project));
                projects_vec.push(projectdata);
            }
            Ok(projects_vec)
        }
        Err(e) => Err(e),
    }
}

pub async fn _get_modules(
    function_endpoint: &Option<String>,
    query: Value,
    read_db: ReadDbGenericFn,
) -> Result<Vec<ModuleResp>, anyhow::Error> {
    read_db(function_endpoint, "modules", &query)
        .await
        .and_then(|items| {
            let mut items = items.clone();
            for v in items.iter_mut() {
                _module_add_missing_fields(v);
            }
            serde_json::from_slice(&serde_json::to_vec(&items).unwrap())
                .map_err(|e| anyhow::anyhow!("Failed to map modules: {}\nResponse: {:?}", e, items))
        })
}

pub async fn _get_module_optional(
    function_endpoint: &Option<String>,
    query: Value,
    read_db: ReadDbGenericFn,
) -> Result<Option<ModuleResp>, anyhow::Error> {
    match _get_modules(function_endpoint, query, read_db).await {
        Ok(mut modules) => {
            if modules.is_empty() {
                Ok(None)
            } else {
                Ok(modules.pop())
            }
        }
        Err(e) => Err(e),
    }
}

pub async fn _get_deployments(
    function_endpoint: &Option<String>,
    query: Value,
    read_db: ReadDbGenericFn,
) -> Result<Vec<DeploymentResp>, anyhow::Error> {
    match read_db(function_endpoint, "deployments", &query).await {
        Ok(items) => {
            let mut items = items.clone();
            _mutate_deployment(&mut items);
            serde_json::from_slice(&serde_json::to_vec(&items).unwrap()).map_err(|e| {
                anyhow::anyhow!(
                    "Failed to deployments: {}\nResponse: {:?}",
                    e.to_string(),
                    items
                )
            })
        }
        Err(e) => Err(e),
    }
}

pub fn _mutate_deployment(value: &mut Vec<Value>) {
    for v in value {
        // Value is an array, loop through every element and modify the deleted field
        v["deleted"] = serde_json::json!(v["deleted"].as_f64().unwrap() != 0.0);
        // Boolean is not supported in GSI, so convert it to/from int for AWS
        _deployment_add_missing_fields(v);
    }
}

pub async fn _get_deployment_and_dependents(
    function_endpoint: &Option<String>,
    query: Value,
    read_db: ReadDbGenericFn,
) -> Result<(Option<DeploymentResp>, Vec<Dependent>), anyhow::Error> {
    match read_db(function_endpoint, "deployments", &query).await {
        Ok(items) => {
            let mut deployments_vec: Vec<DeploymentResp> = vec![];
            let mut dependents_vec: Vec<Dependent> = vec![];
            for e in items {
                if e.get("SK")
                    .unwrap()
                    .as_str()
                    .unwrap()
                    .starts_with("DEPENDENT#")
                {
                    let dependent: Dependent =
                        serde_json::from_value(e.clone()).expect("Failed to parse dependent");
                    dependents_vec.push(dependent);
                } else {
                    let mut value = e.clone();
                    value["deleted"] = serde_json::json!(value["deleted"].as_f64().unwrap() != 0.0); // Boolean is not supported in GSI, so convert it to/from int for AWS
                    _deployment_add_missing_fields(&mut value);
                    let deployment: DeploymentResp =
                        serde_json::from_value(value).expect("Failed to parse deployment");
                    deployments_vec.push(deployment);
                }
            }
            if deployments_vec.is_empty() {
                println!("No deployment was found");
                return Ok((None, dependents_vec));
            }
            Ok((Some(deployments_vec[0].clone()), dependents_vec))
        }
        Err(e) => Err(e),
    }
}

pub async fn _get_deployment(
    function_endpoint: &Option<String>,
    query: Value,
    read_db: ReadDbGenericFn,
) -> Result<Option<DeploymentResp>, anyhow::Error> {
    match _get_deployment_and_dependents(function_endpoint, query, read_db).await {
        Ok((deployment, _)) => Ok(deployment),
        Err(e) => Err(e),
    }
}

pub async fn _get_dependents(
    function_endpoint: &Option<String>,
    query: Value,
    read_db: ReadDbGenericFn,
) -> Result<Vec<Dependent>, anyhow::Error> {
    match _get_deployment_and_dependents(function_endpoint, query, read_db).await {
        Ok((_, dependents)) => Ok(dependents),
        Err(e) => Err(e),
    }
}

pub async fn _get_events(
    function_endpoint: &Option<String>,
    query: Value,
    read_db: ReadDbGenericFn,
) -> Result<Vec<EventData>, anyhow::Error> {
    match read_db(function_endpoint, "events", &query).await {
        Ok(items) => {
            let mut events_vec: Vec<EventData> = vec![];
            for event in items {
                let eventdata: EventData = serde_json::from_value(event.clone())
                    .unwrap_or_else(|_| panic!("Failed to parse event {}", event));
                events_vec.push(eventdata);
            }
            Ok(events_vec)
        }
        Err(e) => Err(e),
    }
}

pub async fn _get_change_records(
    function_endpoint: &Option<String>,
    query: Value,
    read_db: ReadDbGenericFn,
) -> Result<InfraChangeRecord, anyhow::Error> {
    match read_db(function_endpoint, "change_records", &query).await {
        Ok(change_records) => {
            if change_records.len() == 1 {
                let change_record: InfraChangeRecord =
                    serde_json::from_value(change_records[0].clone())
                        .expect("Failed to parse change record");
                Ok(change_record)
            } else if change_records.is_empty() {
                return Err(anyhow::anyhow!("No change record found"));
            } else {
                panic!("Expected exactly one change record");
            }
        }
        Err(e) => Err(e),
    }
}

pub async fn _get_policy(
    function_endpoint: &Option<String>,
    query: Value,
    read_db: ReadDbGenericFn,
) -> Result<PolicyResp, anyhow::Error> {
    match read_db(function_endpoint, "policies", &query).await {
        Ok(items) => {
            if items.len() == 1 {
                let policy: PolicyResp =
                    serde_json::from_value(items[0].clone()).expect("Failed to parse policy");
                Ok(policy)
            } else if items.is_empty() {
                return Err(anyhow::anyhow!("No policy found"));
            } else {
                panic!("Expected exactly one policy");
            }
        }
        Err(e) => Err(e),
    }
}

pub async fn _get_policies(
    function_endpoint: &Option<String>,
    query: Value,
    read_db: ReadDbGenericFn,
) -> Result<Vec<PolicyResp>, anyhow::Error> {
    match read_db(function_endpoint, "policies", &query).await {
        Ok(items) => {
            let mut policies_vec: Vec<PolicyResp> = vec![];
            for policy in items {
                let policydata: PolicyResp = serde_json::from_value(policy.clone())
                    .unwrap_or_else(|_| panic!("Failed to parse policy {}", policy));
                policies_vec.push(policydata);
            }
            Ok(policies_vec)
        }
        Err(e) => Err(e),
    }
}

// If you need to add a field to ModuleResp, you can do it here
fn _module_add_missing_fields(value: &mut Value) {
    if value["cpu"].is_null() {
        value["cpu"] = serde_json::json!("1024")
    };
    if value["cpu"].is_null() {
        value["memory"] = serde_json::json!("2048")
    };
}

// If you need to add a field to DeploymentResp, you can do it here
fn _deployment_add_missing_fields(value: &mut Value) {
    if value["cpu"].is_null() {
        value["cpu"] = serde_json::json!("1024")
    };
    if value["cpu"].is_null() {
        value["memory"] = serde_json::json!("2048")
    };
}
