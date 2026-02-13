// Helper functions

use env_defs::{
    CloudProvider, Dependent, DeploymentResp, EventData, InfraChangeRecord, ModuleResp, PolicyResp,
    ProjectData, ProviderResp,
};
use log::info;
use serde_json::Value;

pub async fn get_projects(
    provider: &dyn CloudProvider,
    query: Value,
) -> Result<Vec<ProjectData>, anyhow::Error> {
    match provider.read_db_generic("config", &query).await {
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
    provider: &dyn CloudProvider,
    query: Value,
) -> Result<Vec<ModuleResp>, anyhow::Error> {
    provider
        .read_db_generic("modules", &query)
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

pub async fn _get_providers(
    provider: &dyn CloudProvider,
    query: Value,
) -> Result<Vec<ProviderResp>, anyhow::Error> {
    provider
        .read_db_generic("modules", &query)
        .await
        .and_then(|items| {
            let mut items = items.clone();
            for v in items.iter_mut() {
                _provider_add_missing_fields(v);
            }
            serde_json::from_slice(&serde_json::to_vec(&items).unwrap()).map_err(|e| {
                anyhow::anyhow!("Failed to map providers: {}\nResponse: {:?}", e, items)
            })
        })
}

pub async fn _get_module_optional(
    provider: &dyn CloudProvider,
    query: Value,
) -> Result<Option<ModuleResp>, anyhow::Error> {
    match _get_modules(provider, query).await {
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

pub async fn _get_provider_optional(
    provider: &dyn CloudProvider,
    query: Value,
) -> Result<Option<ProviderResp>, anyhow::Error> {
    match _get_providers(provider, query).await {
        Ok(mut providers) => {
            if providers.is_empty() {
                Ok(None)
            } else {
                Ok(providers.pop())
            }
        }
        Err(e) => Err(e),
    }
}

pub async fn _get_deployments(
    provider: &dyn CloudProvider,
    query: Value,
) -> Result<Vec<DeploymentResp>, anyhow::Error> {
    match provider.read_db_generic("deployments", &query).await {
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
    provider: &dyn CloudProvider,
    query: Value,
) -> Result<(Option<DeploymentResp>, Vec<Dependent>), anyhow::Error> {
    match provider.read_db_generic("deployments", &query).await {
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
                info!("No deployment was found");
                return Ok((None, dependents_vec));
            }
            Ok((Some(deployments_vec[0].clone()), dependents_vec))
        }
        Err(e) => Err(e),
    }
}

pub async fn _get_deployment(
    provider: &dyn CloudProvider,
    query: Value,
) -> Result<Option<DeploymentResp>, anyhow::Error> {
    match _get_deployment_and_dependents(provider, query).await {
        Ok((deployment, _)) => Ok(deployment),
        Err(e) => Err(e),
    }
}

pub async fn _get_dependents(
    provider: &dyn CloudProvider,
    query: Value,
) -> Result<Vec<Dependent>, anyhow::Error> {
    match _get_deployment_and_dependents(provider, query).await {
        Ok((_, dependents)) => Ok(dependents),
        Err(e) => Err(e),
    }
}

pub async fn _get_events(
    provider: &dyn CloudProvider,
    query: Value,
) -> Result<Vec<EventData>, anyhow::Error> {
    match provider.read_db_generic("events", &query).await {
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
    provider: &dyn CloudProvider,
    query: Value,
) -> Result<InfraChangeRecord, anyhow::Error> {
    match provider.read_db_generic("change_records", &query).await {
        Ok(change_records) => {
            if change_records.len() == 1 {
                let change_record: InfraChangeRecord =
                    serde_json::from_value(change_records[0].clone())
                        .expect("Failed to parse change record");
                Ok(change_record)
            } else if change_records.is_empty() {
                Err(anyhow::anyhow!("No change record found"))
            } else {
                panic!("Expected exactly one change record");
            }
        }
        Err(e) => Err(e),
    }
}

pub async fn _get_policy(
    provider: &dyn CloudProvider,
    query: Value,
) -> Result<PolicyResp, anyhow::Error> {
    match provider.read_db_generic("policies", &query).await {
        Ok(items) => {
            if items.len() == 1 {
                let policy: PolicyResp =
                    serde_json::from_value(items[0].clone()).expect("Failed to parse policy");
                Ok(policy)
            } else if items.is_empty() {
                Err(anyhow::anyhow!("No policy found"))
            } else {
                panic!("Expected exactly one policy");
            }
        }
        Err(e) => Err(e),
    }
}

pub async fn _get_policies(
    provider: &dyn CloudProvider,
    query: Value,
) -> Result<Vec<PolicyResp>, anyhow::Error> {
    match provider.read_db_generic("policies", &query).await {
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
    if value["memory"].is_null() {
        value["memory"] = serde_json::json!("2048")
    };
    if value["reference"].is_null() {
        value["reference"] = serde_json::json!("")
    };
}

// If you need to add a field to ProviderResp, you can do it here
fn _provider_add_missing_fields(_value: &mut Value) {
    // Only here for future proofing
}

// If you need to add a field to DeploymentResp, you can do it here
fn _deployment_add_missing_fields(value: &mut Value) {
    if value["cpu"].is_null() {
        value["cpu"] = serde_json::json!("1024")
    };
    if value["memory"].is_null() {
        value["memory"] = serde_json::json!("2048")
    };
    if value["reference"].is_null() {
        value["reference"] = serde_json::json!("")
    };
}
