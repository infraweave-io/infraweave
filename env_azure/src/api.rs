use std::{env, process::exit};

use anyhow::Result;
use azure_core::credentials::TokenCredential;
use azure_identity::DeveloperToolsCredential;
use env_defs::{
    get_change_record_identifier, get_deployment_identifier, get_event_identifier,
    get_module_identifier, get_policy_identifier, GenericFunctionResponse,
};
use env_utils::{get_epoch, sanitize_payload_for_logging, zero_pad_semver};
use log::{error, info};
use reqwest::Client;
use serde_json::{json, Value};

use crate::custom::CustomImdsCredential;

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
    function_endpoint: &Option<String>,
    payload: &Value,
    project_id: &str,
    region: &str,
) -> Result<GenericFunctionResponse> {
    let api_environment = match std::env::var("INFRAWEAVE_ENV") {
        Ok(env) => env,
        Err(_) => {
            eprintln!("Please make sure to set the platform environment, for example: \"export INFRAWEAVE_ENV=dev\"");
            exit(1);
            // TODO: Remove unwraps in cli and then throw error instead of exit(1)
            // return Err(CloudHandlerError::MissingEnvironment());
        }
    };
    let subscription_id = project_id;
    let base_url = function_endpoint.clone().unwrap_or_else(|| {
        let truncated_subscription_id = &subscription_id[..18.min(subscription_id.len())];
        format!(
            "https://iw-{}-{}-{}.azurewebsites.net",
            truncated_subscription_id, region, api_environment
        )
    });

    let function_url = format!("{}/api/api", base_url);

    let scope = format!(
        "api://infraweave-broker-{}-{}-{}/.default",
        subscription_id, api_environment, region
    );

    let token = if env::var("TEST_MODE").is_ok() {
        let token = "TEST_TOKEN";
        token.to_string()
    } else if env::var("AZURE_CONTAINER_INSTANCE").is_ok() {
        let credential = CustomImdsCredential::new();
        credential
            .get_token(&[&scope], None)
            .await?
            .token
            .secret()
            .to_string()
    } else {
        match DeveloperToolsCredential::new(None)?
            .get_token(&[&scope], None)
            .await
        {
            Ok(token) => token.token.secret().to_owned(),
            Err(e) => {
                error!("Failed to get token for scope {}: {}", &scope, e);
                "error".to_string()
            }
        }
    };

    // Convert payload to a JSON string and log sanitized payload
    let serialized_payload = serde_json::to_vec(&payload)?;
    let sanitized_payload = sanitize_payload_for_logging(payload.clone());
    info!(
        "Invoking Azure Function with payload: {}",
        serde_json::to_string(&sanitized_payload).unwrap()
    );

    let client = Client::new();
    // println!("Function URL: {}", function_url);
    // println!("bearer_auth: {}", token.token.secret());
    eprintln!(
        "serialized_payload: {}",
        String::from_utf8(serialized_payload.clone()).unwrap()
    );
    let response = client
        .post(function_url)
        .bearer_auth(token) // Use the Bearer token for authorization
        .header("Content-Type", "application/json")
        .body(serialized_payload)
        .send()
        .await;

    match response {
        Ok(res) => {
            let status = res.status();
            let response_string = res.text().await?;

            eprintln!("Response status: {}", status);
            eprintln!("Function response: {}", response_string);
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
    project_id: &str,
    region: &str,
) -> Result<GenericFunctionResponse, anyhow::Error> {
    let full_query = env_defs::read_db_event(table, query);
    run_function(function_endpoint, &full_query, project_id, region).await
}

pub fn get_latest_module_version_query(module: &str, track: &str) -> Value {
    _get_latest_module_version_query("LATEST_MODULE", module, track)
}

pub fn get_latest_stack_version_query(stack: &str, track: &str) -> Value {
    _get_latest_module_version_query("LATEST_STACK", stack, track)
}

pub fn get_latest_provider_version_query(provider: &str) -> Value {
    _get_latest_provider_version_query("LATEST_PROVIDER", provider)
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

fn _get_latest_provider_version_query(pk: &str, provider: &str) -> Value {
    let sk: String = format!("PROVIDER#{}", provider);
    json!({
        "query": "SELECT * FROM c WHERE c.PK = @pk AND c.SK = @sk",
        "parameters": [
            { "name": "@pk", "value": pk },
            { "name": "@sk", "value": sk }
        ]
    })
}

pub fn get_all_latest_modules_query(
    track: &str,
    include_deprecated: bool,
    include_dev000: bool,
) -> Value {
    _get_all_latest_modules_query("LATEST_MODULE", track, include_deprecated, include_dev000)
}

pub fn get_all_latest_stacks_query(
    track: &str,
    include_deprecated: bool,
    include_dev000: bool,
) -> Value {
    _get_all_latest_modules_query("LATEST_STACK", track, include_deprecated, include_dev000)
}

pub fn get_all_latest_providers_query() -> Value {
    _get_all_latest_providers_query("LATEST_PROVIDER")
}

fn _get_all_latest_modules_query(
    pk: &str,
    track: &str,
    include_deprecated: bool,
    include_dev000: bool,
) -> Value {
    let mut query_str = String::from("SELECT * FROM c WHERE c.PK = @pk");
    let mut parameters = vec![json!({ "name": "@pk", "value": pk })];

    if !track.is_empty() {
        query_str.push_str(" AND STARTSWITH(c.SK, @prefix)");
        parameters.push(json!({ "name": "@prefix", "value": format!("MODULE#{}::", track) }));
    }

    if !include_deprecated {
        query_str.push_str(" AND (NOT IS_DEFINED(c.deprecated) OR c.deprecated = false)");
    }

    if !include_dev000 {
        query_str.push_str(" AND (NOT STARTSWITH(c.version, '0.0.0-dev'))");
    }

    json!({
        "query": query_str,
        "parameters": parameters
    })
}

fn _get_all_latest_providers_query(pk: &str) -> Value {
    json!({
        "query": "SELECT * FROM c WHERE c.PK = @pk",
        "parameters": [
            { "name": "@pk", "value": pk }
        ]
    })
}

pub fn get_all_module_versions_query(
    module: &str,
    track: &str,
    include_deprecated: bool,
    include_dev000: bool,
) -> Value {
    _get_all_module_versions_query(module, track, include_deprecated, include_dev000)
}

pub fn get_all_stack_versions_query(
    stack: &str,
    track: &str,
    include_deprecated: bool,
    include_dev000: bool,
) -> Value {
    _get_all_module_versions_query(stack, track, include_deprecated, include_dev000)
}

fn _get_all_module_versions_query(
    module: &str,
    track: &str,
    include_deprecated: bool,
    include_dev000: bool,
) -> Value {
    let id: String = format!("MODULE#{}", get_module_identifier(module, track));
    let mut query_str =
        String::from("SELECT * FROM c WHERE c.PK = @id AND STARTSWITH(c.SK, @prefix)");
    let parameters = vec![
        json!({ "name": "@id", "value": id }),
        json!({ "name": "@prefix", "value": "VERSION#" }),
    ];

    if !include_deprecated {
        query_str.push_str(" AND (NOT IS_DEFINED(c.deprecated) OR c.deprecated = false)");
    }

    if !include_dev000 {
        query_str.push_str(" AND (NOT STARTSWITH(c.version, '0.0.0-dev'))");
    }

    json!({
        "query": query_str,
        "parameters": parameters,
    })
}

pub fn get_module_version_query(module: &str, track: &str, version: &str) -> Value {
    let id: String = format!("MODULE#{}", get_module_identifier(module, track));
    let version_id = format!("VERSION#{}", zero_pad_semver(version, 3).unwrap());
    json!({
        "query": "SELECT TOP 1 * FROM c WHERE c.PK = @id AND c.SK = @version_id",
        "parameters": [
            { "name": "@id", "value": id },
            { "name": "@version_id", "value": version_id }
        ]
    })
}

pub fn get_provider_version_query(provider: &str, version: &str) -> Value {
    let id: String = format!("PROVIDER#{}", provider);
    let version_id = format!("VERSION#{}", zero_pad_semver(version, 3).unwrap());
    json!({
        "query": "SELECT TOP 1 * FROM c WHERE c.PK = @id AND c.SK = @version_id",
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

pub fn get_all_deployments_query(
    project_id: &str,
    region: &str,
    environment: &str,
    include_deleted: bool,
) -> Value {
    if include_deleted {
        json!({
            "query": "SELECT * FROM c WHERE STARTSWITH(c.PK, @deployment_prefix) AND c.SK = @metadata",
            "parameters": [
                {
                    "name": "@deployment_prefix",
                    "value": format!("DEPLOYMENT#{}", get_deployment_identifier(project_id, region, "", environment))
                },
                {
                    "name": "@metadata",
                    "value": "METADATA"
                }
            ]
        })
    } else {
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
}

pub fn get_deployment_and_dependents_query(
    project_id: &str,
    region: &str,
    deployment_id: &str,
    environment: &str,
    include_deleted: bool,
) -> Value {
    if include_deleted {
        json!({
            "query": "SELECT * FROM c WHERE c.PK = @pk",
            "parameters": [
                {
                    "name": "@pk",
                    "value": format!("DEPLOYMENT#{}", get_deployment_identifier(project_id, region, deployment_id, environment))
                }
            ]
        })
    } else {
        json!({
            "query": "SELECT * FROM c WHERE c.PK = @pk AND c.deleted != @deleted",
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
}

pub fn get_deployment_query(
    project_id: &str,
    region: &str,
    deployment_id: &str,
    environment: &str,
    include_deleted: bool,
) -> Value {
    let pk = format!(
        "DEPLOYMENT#{}",
        get_deployment_identifier(project_id, region, deployment_id, environment)
    );

    if include_deleted {
        json!({
            "query": "SELECT * FROM c WHERE c.PK = @pk AND c.SK = @metadata",
            "parameters": [
                { "name": "@pk", "value": pk },
                { "name": "@metadata", "value": "METADATA" }
            ]
        })
    } else {
        json!({
            "query": "SELECT * FROM c WHERE c.PK = @pk AND c.SK = @metadata AND c.deleted = @deleted",
            "parameters": [
                { "name": "@pk", "value": pk },
                { "name": "@metadata", "value": "METADATA" },
                { "name": "@deleted", "value": 0 }
            ]
        })
    }
}

// TODO: Add environment_refiner parameter to query
pub fn get_deployments_using_module_query(
    project_id: &str,
    region: &str,
    module: &str,
    environment: &str,
    include_deleted: bool,
) -> Value {
    let _environment_refiner = if environment.is_empty() {
        ""
    } else if environment.contains('/') {
        &format!("{}::", environment)
    } else {
        &format!("{}/", environment)
    };

    if include_deleted {
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
                        "DEPLOYMENT#{}",
                        get_deployment_identifier(project_id, region, "", environment)
                    )
                },
                {
                    "name": "@metadata",
                    "value": "METADATA"
                }
            ]
        })
    } else {
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

pub fn get_deployment_history_plans_query(
    project_id: &str,
    region: &str,
    environment: Option<&str>,
) -> Value {
    let pk_prefix = if let Some(env) = environment {
        format!(
            "PLAN#{}",
            get_deployment_identifier(project_id, region, "", env)
        )
    } else {
        format!("PLAN#{}::{}", project_id, region)
    };

    json!({
        "query": "SELECT * FROM c WHERE STARTSWITH(c.PK, @pk_prefix) AND c.deleted = @not_deleted ORDER BY c.epoch DESC",
        "parameters": [
            {
                "name": "@pk_prefix",
                "value": pk_prefix
            },
            {
                "name": "@not_deleted",
                "value": 0
            }
        ]
    })
}

pub fn get_deployment_history_deleted_query(
    project_id: &str,
    region: &str,
    environment: Option<&str>,
) -> Value {
    let pk_prefix = if let Some(env) = environment {
        format!(
            "DEPLOYMENT#{}",
            get_deployment_identifier(project_id, region, "", env)
        )
    } else {
        format!("DEPLOYMENT#{}::{}", project_id, region)
    };

    json!({
        "query": "SELECT * FROM c WHERE STARTSWITH(c.PK, @pk_prefix) AND c.deleted = @deleted AND c.SK = @metadata ORDER BY c.epoch DESC",
        "parameters": [
            {
                "name": "@pk_prefix",
                "value": pk_prefix
            },
            {
                "name": "@deleted",
                "value": 1
            },
            {
                "name": "@metadata",
                "value": "METADATA"
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
        "query": "SELECT VALUE udf.getAllProjects()[0]",
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
    event_type: Option<&str>,
) -> Value {
    let mut query_str = "SELECT * FROM c WHERE c.PK = @pk".to_string();
    let mut params = vec![
        json!({ "name": "@pk", "value": format!("EVENT#{}", get_event_identifier(project_id, region, deployment_id, environment))}),
    ];

    if let Some(etype) = event_type {
        if etype == "mutate" {
            query_str.push_str(" AND (c.event = 'apply' OR c.event = 'destroy')");
        } else if etype == "plan" {
            query_str.push_str(" AND c.event = 'plan'");
        } else if !etype.is_empty() {
            query_str.push_str(" AND c.event = @event");
            params.push(json!({ "name": "@event", "value": etype }));
        }
    }

    query_str.push_str(" ORDER BY c.SK DESC");

    json!({
        "query": query_str,
        "parameters": params
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
        "query": "SELECT TOP 1 * FROM c WHERE c.PK = @policy AND c.SK = @version",
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

pub fn get_project_map_query() -> Value {
    json!({
        "query": "SELECT udf.getProjectMap() AS data",
    })
}

pub fn get_all_regions_query() -> Value {
    json!({
        "query": "SELECT udf.getAllRegions() AS data",
    })
}
