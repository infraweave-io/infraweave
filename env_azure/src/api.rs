use std::{future::Future, pin::Pin};

use anyhow::Result;
use azure_core::auth::TokenCredential;
use azure_identity::AzureCliCredential;
use env_defs::{
    get_change_record_identifier, get_deployment_identifier, get_event_identifier,
    get_module_identifier, get_policy_identifier, GenericCloudConfig, GenericFunctionResponse,
};
use env_utils::{get_epoch, sanitize_payload_for_logging, zero_pad_semver};
use log::{error, info};
use reqwest::Client;
use serde_json::{json, Value};

#[derive(Clone)]
pub struct AzureConfig {
    lambda_endpoint_url: Option<String>,
}

impl GenericCloudConfig for AzureConfig {
    fn default() -> Self {
        AzureConfig {
            lambda_endpoint_url: None,
        }
    }

    fn custom(lambda_endpoint_url: &str) -> Self {
        AzureConfig {
            lambda_endpoint_url: Some(lambda_endpoint_url.to_string()),
        }
    }

    fn get_function_endpoint(&self) -> Option<String> {
        self.lambda_endpoint_url.clone()
    }
}

pub async fn get_project_id() -> Result<String, anyhow::Error> {
    let subscription_id =
        std::env::var("AZURE_SUBSCRIPTION_ID").expect("AZURE_SUBSCRIPTION_ID not set");

    Ok(subscription_id.to_string())
}

pub async fn get_user_id() -> Result<String, anyhow::Error> {
    let user_id = "generic_user_id"; // TODO: implement

    Ok(user_id.to_string())
}

pub async fn run_function(
    _function_endpoint: &Option<String>,
    payload: &Value,
) -> Result<GenericFunctionResponse> {
    let base_url = format!(
        "https://infraweave-func-{}.azurewebsites.net",
        std::env::var("AZURE_SUBSCRIPTION_ID").expect("AZURE_SUBSCRIPTION_ID not set")
    );

    let function_key = std::env::var("AZURE_FUNCTION_KEY").expect("AZURE_FUNCTION_KEY not set");
    let function_url = format!("{}/api/api?code={}", base_url, function_key);

    let credential = AzureCliCredential::default(); //DefaultAzureCredential::create(TokenCredentialOptions::default()).unwrap();
    let token = match credential
        .get_token(&["https://management.azure.com/.default"])
        .await
    {
        Ok(token) => token,
        Err(e) => {
            error!("Failed to get Azure token: {}", e);
            return Err(anyhow::anyhow!("Failed to get Azure token: {}", e));
        }
    };

    // Convert payload to a JSON string and log sanitized payload
    let serialized_payload = serde_json::to_vec(&payload)?;
    let sanitized_payload = sanitize_payload_for_logging(payload.clone());
    info!(
        "Invoking Azure Function with payload: {}",
        serde_json::to_string_pretty(&sanitized_payload).unwrap()
    );

    let client = Client::new();
    // println!("Function URL: {}", function_url);
    // println!("bearer_auth: {}", token.token.secret());
    println!(
        "serialized_payload: {}",
        String::from_utf8(serialized_payload.clone()).unwrap()
    );
    let response = client
        .post(function_url)
        .bearer_auth(token.token.secret()) // Use the Bearer token for authorization
        .header("Content-Type", "application/json")
        .body(serialized_payload)
        .send()
        .await;

    match response {
        Ok(res) => {
            let status = res.status();
            let response_string = res.text().await?;

            println!("Response status: {}", status);
            println!("Function response: {}", response_string);
            let parsed_json: Value =
                serde_json::from_str(&response_string).expect("response not valid JSON");

            Ok(GenericFunctionResponse {
                payload: parsed_json,
            })
        }
        Err(e) => {
            error!("Failed to invoke Azure Function: {}", e);
            Err(anyhow::anyhow!(format!(
                "Failed to invoke Azure Function: {}",
                e
            )))
        }
    }
}

pub async fn read_db(
    function_endpoint: &Option<String>,
    table: &str,
    query: &Value,
) -> Result<GenericFunctionResponse, anyhow::Error> {
    let full_query = json!({
        "event": "read_db",
        "table": table,
        "data": {
            "query": query
        }
    });
    run_function(function_endpoint, &full_query).await
}

pub fn read_db_generic(
    function_endpoint: &Option<String>,
    table: &str,
    query: &Value,
) -> Pin<Box<dyn Future<Output = Result<Vec<Value>, anyhow::Error>> + Send>> {
    let table = table.to_string();
    let query = query.clone();
    let function_endpoint = function_endpoint.clone();
    Box::pin(async move {
        match read_db(&function_endpoint, &table, &query).await {
            Ok(response) => {
                let items = response.payload.as_array().unwrap_or(&vec![]).clone();
                Ok(items.clone())
            }
            Err(e) => Err(e),
        }
    })
}

pub fn get_latest_module_version_query(module: &str, track: &str) -> Value {
    _get_latest_module_version_query("LATEST_MODULE", module, track)
}

pub fn get_latest_stack_version_query(stack: &str, track: &str) -> Value {
    _get_latest_module_version_query("LATEST_STACK", stack, track)
}

fn _get_latest_module_version_query(pk: &str, module: &str, track: &str) -> Value {
    let sk: String = format!("MODULE#{}", get_module_identifier(module, track));
    json!({
        "query": "SELECT * FROM c WHERE c.PK = @pk AND c.SK = @sk",
        "parameters": [
            { "name": "@pk", "value": pk },
            { "name": "@sk", "value": sk }
        ]
    })
}

pub fn get_generate_presigned_url_query(key: &str, bucket: &str) -> Value {
    json!({
        "event": "generate_presigned_url",
            "data":{
            "key": key,
            "bucket_name": bucket,
            "expires_in": 60,
        }
    })
}

pub fn get_all_latest_modules_query(track: &str) -> Value {
    _get_all_latest_modules_query("LATEST_MODULE", track)
}

pub fn get_all_latest_stacks_query(track: &str) -> Value {
    _get_all_latest_modules_query("LATEST_STACK", track)
}

fn _get_all_latest_modules_query(pk: &str, track: &str) -> Value {
    if track.is_empty() {
        json!({
            "query": "SELECT * FROM c WHERE c.PK = @pk",
            "parameters": [
                { "name": "@pk", "value": pk }
            ]
        })
    } else {
        json!({
            "query": "SELECT * FROM c WHERE c.PK = @pk AND STARTSWITH(c.SK, @prefix)",
            "parameters": [
                { "name": "@pk", "value": pk },
                { "name": "@prefix", "value": format!("MODULE#{}::", track) }
            ]
        })
    }
}

pub fn get_all_module_versions_query(module: &str, track: &str) -> Value {
    _get_all_module_versions_query(module, track)
}

pub fn get_all_stack_versions_query(stack: &str, track: &str) -> Value {
    _get_all_module_versions_query(stack, track)
}

fn _get_all_module_versions_query(module: &str, track: &str) -> Value {
    let id: String = format!("MODULE#{}", get_module_identifier(module, track));
    json!({
        "query": "SELECT * FROM c WHERE c.PK = @id AND STARTSWITH(c.SK, @prefix)",
        "parameters": [
            { "name": "@id", "value": id },
            { "name": "@prefix", "value": "VERSION#" }
        ]
    })
}

pub fn get_module_version_query(module: &str, track: &str, version: &str) -> Value {
    let id: String = format!("MODULE#{}", get_module_identifier(module, track));
    let version_id = format!("VERSION#{}", zero_pad_semver(version, 3).unwrap());
    json!({
        "query": "SELECT * FROM c WHERE c.PK = @id AND c.SK = @version_id LIMIT 1",
        "parameters": [
            { "name": "@id", "value": id },
            { "name": "@version_id", "value": version_id }
        ]
    })
}

pub fn get_stack_version_query(module: &str, track: &str, version: &str) -> Value {
    get_module_version_query(module, track, version)
}

// Deployment

pub fn get_all_deployments_query(project_id: &str, region: &str, environment: &str) -> Value {
    json!({
        "query": "SELECT * FROM c WHERE c.deleted_PK_base = @deleted_PK_base AND STARTSWITH(c.PK, @deployment_prefix)",
        "parameters": [
            {
                "name": "@deleted_PK_base",
                "value": format!("0|DEPLOYMENT#{}", get_deployment_identifier(project_id, region, "", ""))
            },
            {
                "name": "@deployment_prefix",
                "value": format!("DEPLOYMENT#{}", get_deployment_identifier(project_id, region, "", environment))
            }
        ]
    })
}

// TODO: Add include_deleted parameter to query
pub fn get_deployment_and_dependents_query(
    project_id: &str,
    region: &str,
    deployment_id: &str,
    environment: &str,
    _include_deleted: bool,
) -> Value {
    json!({
        "query": "SELECT * FROM c WHERE c.PK = @pk AND c.deleted <> @deleted",
        "parameters": [
            {
                "name": "@pk",
                "value": format!("DEPLOYMENT#{}", get_deployment_identifier(project_id, region, deployment_id, environment))
            },
            {
                "name": "@deleted",
                "value": 1
            }
        ]
    })
}

// TODO: Add include_deleted parameter to query
pub fn get_deployment_query(
    project_id: &str,
    region: &str,
    deployment_id: &str,
    environment: &str,
    _include_deleted: bool,
) -> Value {
    json!({
        "query": "SELECT * FROM c WHERE c.PK = @pk AND c.SK = @metadata AND c.deleted = @deleted",
        "parameters": [
            {
                "name": "@pk",
                "value": format!("DEPLOYMENT#{}", get_deployment_identifier(project_id, region, deployment_id, environment))
            },
            {
                "name": "@metadata",
                "value": "METADATA"
            },
            {
                "name": "@deleted",
                "value": 0
            }
        ]
    })
}

// TODO: Add environment_refiner parameter to query
pub fn get_deployments_using_module_query(
    project_id: &str,
    region: &str,
    module: &str,
    environment: &str,
) -> Value {
    let _environment_refiner = if environment.is_empty() {
        ""
    } else if environment.contains('/') {
        &format!("{}::", environment)
    } else {
        &format!("{}/", environment)
    };
    json!({
        "query": "SELECT * FROM c WHERE c.module_PK_base = @module AND STARTSWITH(c.deleted_PK, @deployment_prefix) AND c.SK = @metadata",
        "parameters": [
            {
                "name": "@module",
                "value": format!(
                    "MODULE#{}#{}",
                    get_deployment_identifier(project_id, region, "", ""),
                    module
                )
            },
            {
                "name": "@deployment_prefix",
                "value": format!(
                    "0|DEPLOYMENT#{}",
                    get_deployment_identifier(project_id, region, "", environment)
                )
            },
            {
                "name": "@metadata",
                "value": "METADATA"
            }
        ]
    })
}

pub fn get_plan_deployment_query(
    project_id: &str,
    region: &str,
    deployment_id: &str,
    environment: &str,
    job_id: &str,
) -> Value {
    json!({
        "query": "SELECT * FROM c WHERE c.PK = @pk AND c.SK = @job_id AND c.deleted <> @deleted",
        "parameters": [
            {
                "name": "@pk",
                "value": format!("PLAN#{}", get_deployment_identifier(project_id, region, deployment_id, environment))
            },
            {
                "name": "@job_id",
                "value": job_id
            },
            {
                "name": "@deleted",
                "value": 1
            }
        ]
    })
}

pub fn get_dependents_query(
    project_id: &str,
    region: &str,
    deployment_id: &str,
    environment: &str,
) -> Value {
    json!({
        "query": "SELECT * FROM c WHERE c.PK = @pk AND STARTSWITH(c.SK, @dependent_prefix) AND c.deleted = @deleted",
        "parameters": [
            {
                "name": "@pk",
                "value": format!("DEPLOYMENT#{}", get_deployment_identifier(project_id, region, deployment_id, environment))
            },
            {
                "name": "@dependent_prefix",
                "value": "DEPENDENT#"
            },
            {
                "name": "@deleted",
                "value": 0
            }
        ]
    })
}

pub fn get_deployments_to_driftcheck_query(project_id: &str, region: &str) -> Value {
    json!({
        "query": "SELECT * FROM c WHERE c.deleted_SK_base = @deleted_SK_base AND c.next_drift_check_epoch BETWEEN @start_epoch AND @current_epoch",
        "parameters": [
            {
                "name": "@deleted_SK_base",
                "value": format!("0|METADATA#{}", get_deployment_identifier(project_id, region, "", ""))
            },
            {
                "name": "@start_epoch",
                "value": 0
            },
            {
                "name": "@current_epoch",
                "value": get_epoch()
            }
        ]
    })
}

pub fn get_all_projects_query() -> Value {
    // Only available using central role
    json!({
        "query": "SELECT * FROM c WHERE c.PK = @PK",
        "parameters": [
            { "name": "@PK", "value": "PROJECTS" }
        ]
    })
}

pub fn get_current_project_query(project_id: &str) -> Value {
    json!({
        "query": "SELECT * FROM c WHERE c.SK = @sk",
        "parameters": [
            { "name": "@sk", "value": format!("PROJECT#{}", project_id) }
        ]
    })
}

// Event

pub fn get_events_query(
    project_id: &str,
    region: &str,
    deployment_id: &str,
    environment: &str,
) -> Value {
    json!({
        "query": "SELECT * FROM c WHERE c.PK = @pk",
        "parameters": [
            { "name": "@pk", "value": format!("EVENT#{}", get_event_identifier(project_id, region, deployment_id, environment))}
        ]
    })
}

pub fn get_all_events_between_query(region: &str, start_epoch: u128, end_epoch: u128) -> Value {
    json!({
        "query": "SELECT * FROM c WHERE c.PK_base_region = @pk_base_region AND c.SK BETWEEN @start_epoch AND @end_epoch",
        "parameters": [
            { "name": "@pk_base_region", "value": format!("EVENT#{}", region) },
            { "name": "@start_epoch", "value": start_epoch.to_string() },
            { "name": "@end_epoch", "value": end_epoch.to_string() }
        ]
    })
}

// Change record

pub fn get_change_records_query(
    project_id: &str,
    region: &str,
    environment: &str,
    deployment_id: &str,
    job_id: &str,
    change_type: &str,
) -> Value {
    json!({
        "query": "SELECT * FROM c WHERE c.PK = @pk AND c.SK = @sk",
        "parameters": [
            {
                "name": "@pk",
                "value": format!("{}#{}", change_type, get_change_record_identifier(project_id, region, deployment_id, environment))
            },
            {
                "name": "@sk",
                "value": job_id
            }
        ]
    })
}

// Policy

pub fn get_newest_policy_version_query(policy: &str, environment: &str) -> Value {
    json!({
        "query": "SELECT TOP 1 * FROM c WHERE c.PK = @policy ORDER BY c._ts DESC",
        "parameters": [
            {
                "name": "@policy",
                "value": format!("POLICY#{}", get_policy_identifier(policy, environment))
            }
        ]
    })
}

pub fn get_all_policies_query(environment: &str) -> Value {
    json!({
        "query": "SELECT * FROM c WHERE c.PK = @current AND STARTSWITH(c.SK, @policy_prefix)",
        "parameters": [
            { "name": "@current", "value": "CURRENT" },
            { "name": "@policy_prefix", "value": format!("POLICY#{}", environment) }
        ]
    })
}

pub fn get_policy_query(policy: &str, environment: &str, version: &str) -> Value {
    json!({
        "query": "SELECT * FROM c WHERE c.PK = @policy AND c.SK = @version LIMIT 1",
        "parameters": [
            {
                "name": "@policy",
                "value": format!("POLICY#{}", get_policy_identifier(policy, environment))
            },
            {
                "name": "@version",
                "value": format!("VERSION#{}", zero_pad_semver(version, 3).unwrap())
            }
        ]
    })
}
