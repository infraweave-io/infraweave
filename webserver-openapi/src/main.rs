mod structs;
use axum::extract::Path;
use axum::response::IntoResponse;
use axum::{Json, Router};

use axum_macros::debug_handler;

use env_common::ModuleEnvironmentHandler;
use hyper::StatusCode;
use serde_json::json;
use std::io::Error;
use std::net::{Ipv4Addr, SocketAddr};
use structs::{DependantsV1, DependencyV1, DeploymentV1, EventData, ModuleV1, PolicyV1};
use tokio::net::TcpListener;
use utoipa::{
    openapi::security::{ApiKey, ApiKeyValue, SecurityScheme},
    Modify, OpenApi,
};
use utoipa_rapidoc::RapiDoc;
use utoipa_redoc::{Redoc, Servable};
use utoipa_scalar::{Scalar, Servable as ScalarServable};
use utoipa_swagger_ui::SwaggerUi;

#[derive(OpenApi)]
#[openapi(
    paths(describe_deployment, get_event_data, get_modules, get_deployments, read_logs, get_policies, get_policy_version),
    components(schemas(EventData, ModuleV1, DeploymentV1, PolicyV1)),
    modifiers(&SecurityAddon),
    tags(
        (name = "api", description = "API for custom structs")
    )
)]
struct ApiDoc;

struct SecurityAddon;

impl Modify for SecurityAddon {
    fn modify(&self, openapi: &mut utoipa::openapi::OpenApi) {
        if let Some(components) = openapi.components.as_mut() {
            // components.add_security_scheme(
            //     "api_key",
            //     SecurityScheme::ApiKey(ApiKey::Header(ApiKeyValue::new("todo_apikey"))),
            // )
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    let app = Router::new()
        .merge(SwaggerUi::new("/swagger-ui").url("/api-docs/openapi.json", ApiDoc::openapi()))
        .merge(Redoc::with_url("/redoc", ApiDoc::openapi()))
        .merge(RapiDoc::new("/api-docs/openapi.json").path("/rapidoc"))
        .merge(Scalar::with_url("/scalar", ApiDoc::openapi()))
        .route(
            "/api/v1/module/:module_name/:module_version",
            axum::routing::get(get_module_version),
        )
        .route(
            "/api/v1/policy/:policy_name/:policy_version",
            axum::routing::get(get_policy_version),
        )
        .route(
            "/api/v1/deployment/:environment/:deployment_id",
            axum::routing::get(describe_deployment),
        )
        .route(
            "/api/v1/logs/:environment/:deployment_id",
            axum::routing::get(read_logs),
        )
        .route(
            "/api/v1/events/:environment/:deployment_id",
            axum::routing::get(get_events),
        )
        .route("/api/v1/modules", axum::routing::get(get_modules))
        .route("/api/v1/policies", axum::routing::get(get_policies))
        .route("/api/v1/deployments", axum::routing::get(get_deployments))
        .route("/api/v1/event_data", axum::routing::get(get_event_data));

    let address = SocketAddr::from((Ipv4Addr::UNSPECIFIED, 8081));
    let listener = TcpListener::bind(&address).await?;
    axum::serve(listener, app.into_make_service()).await
}

#[utoipa::path(
    get,
    path = "/api/v1/deployment/{environment}/{deployment_id}",
    responses(
        (status = 200, description = "Get DeploymentV1", body = Option<DeploymentV1, serde_json::Value>)
    ),
    params(
        ("deployment_id" = str, Path, description = "Deployment id that you want to see"),
        ("environment" = str, Path, description = "Environment of the deployment")
    ),
    description = "Describe DeploymentV1"
)]
async fn describe_deployment(
    Path((environment, deployment_id)): Path<(String, String)>,
) -> impl IntoResponse {
    let region = "eu-central-1".to_string();

    let handler = env_common::AwsHandler {}; // Temporary, will be replaced with get_handler()

    let (deployment, dependents) = match handler
        .describe_deployment_id(&deployment_id, &environment)
        .await
    {
        Ok(deployment) => deployment,
        Err(e) => {
            let error_json = json!({"error": format!("{:?}", e)});
            return (StatusCode::NOT_FOUND, Json(error_json)).into_response();
        }
    };

    let deployment_v1 = DeploymentV1 {
        environment: deployment.environment.clone(),
        epoch: deployment.epoch.clone(),
        deployment_id: deployment.deployment_id.clone(),
        status: deployment.status.clone(),
        module: deployment.module.clone(),
        module_version: deployment.module_version.clone(),
        variables: deployment.variables.clone(),
        output: deployment.output.clone(),
        policy_results: deployment.policy_results.clone(),
        error_text: deployment.error_text.clone(),
        deleted: deployment.deleted.clone(),
        dependencies: deployment.dependencies.iter().map(|d| {
            DependencyV1 {
                deployment_id: d.deployment_id.clone(),
                environment: d.environment.clone(),
            }}).collect(),
        dependants: dependents.iter().map(|d| {
            DependantsV1 {
                deployment_id: d.dependent_id.clone(),
                environment: d.environment.clone(),
            }}).collect(),
    };

    (StatusCode::OK, Json(deployment_v1)).into_response()
}

#[utoipa::path(
    get,
    path = "/api/v1/module/{module_name}/{module_version}",
    responses(
        (status = 200, description = "Get module", body = Option<ModuleV1, serde_json::Value>)
    ),
    params(
        ("module_name" = str, Path, description = "Module name that you want to see"),
        ("module_version" = str, Path, description = "Module version that you want to see"),
    ),
    description = "Get module version"
)]
async fn get_module_version(
    Path((module_name, module_version)): Path<(String, String)>,
) -> impl IntoResponse {
    let handler = env_common::AwsHandler {}; // Temporary, will be replaced with get_handler()

    let module = match handler
        .get_module_version(&module_name, &module_version)
        .await
    {
        Ok(deployment) => deployment,
        Err(e) => {
            let error_json = json!({"error": format!("{:?}", e)});
            return (StatusCode::NOT_FOUND, Json(error_json)).into_response();
        }
    };

    let response = ModuleV1 {
        environment: module.environment.clone(),
        environment_version: module.environment_version.clone(),
        version: module.version.clone(),
        timestamp: module.timestamp.clone(),
        module_name: module.module_name.clone(),
        module: module.module.clone(),
        description: module.description.clone(),
        reference: module.reference.clone(),
        manifest: module.manifest.clone(),
        tf_variables: module.tf_variables.clone(),
        tf_outputs: module.tf_outputs.clone(),
        s3_key: module.s3_key.clone(),
    };

    (StatusCode::OK, Json(response)).into_response()
}


#[utoipa::path(
    get,
    path = "/api/v1/policy/{policy_name}/{policy_version}",
    responses(
        (status = 200, description = "Get policy", body = Option<PolicyV1, serde_json::Value>)
    ),
    params(
        ("policy_name" = str, Path, description = "Policy name that you want to see"),
        ("policy_version" = str, Path, description = "Policy version that you want to see"),
    ),
    description = "Get policy version"
)]
async fn get_policy_version(
    Path((policy_name, policy_version)): Path<(String, String)>,
) -> impl IntoResponse {
    let handler = env_common::AwsHandler {}; // Temporary, will be replaced with get_handler()

    let environment = "dev".to_string();

    let policy = match handler
        .get_policy_version(&policy_name, &environment, &policy_version)
        .await
    {
        Ok(policy) => policy,
        Err(e) => {
            let error_json = json!({"error": format!("{:?}", e)});
            return (StatusCode::NOT_FOUND, Json(error_json)).into_response();
        }
    };

    let response = PolicyV1 {
        environment: policy.environment.clone(),
        environment_version: policy.environment_version.clone(),
        version: policy.version.clone(),
        timestamp: policy.timestamp.clone(),
        policy_name: policy.policy_name.clone(),
        policy: policy.policy.clone(),
        description: policy.description.clone(),
        reference: policy.reference.clone(),
        manifest: policy.manifest.clone(),
        s3_key: policy.s3_key.clone(),
        data: policy.data.clone(),
    };

    (StatusCode::OK, Json(response)).into_response()
}

#[utoipa::path(
    get,
    path = "/api/v1/logs/{environment}/{job_id}",
    responses(
        (status = 200, description = "Get logs", body = serde_json::Value)
    ),
    params(
        ("job_id" = str, Path, description = "Job id that you want to see"),
        ("environment" = str, Path, description = "Environment of the deployment")
    ),
    description = "Describe DeploymentV1"
)]
async fn read_logs(
    Path((environment, job_id)): Path<(String, String)>,
) -> impl IntoResponse {
    let region = "eu-central-1".to_string();

    let handler = env_common::AwsHandler {}; // Temporary, will be replaced with get_handler()

    let deployment = match handler.read_logs(&job_id).await {
        Ok(deployment) => deployment,
        Err(e) => {
            let error_json = json!({"error": format!("{:?}", e)});
            return (StatusCode::NOT_FOUND, Json(error_json)).into_response();
        }
    };

    Json(json!({"logs": deployment})).into_response()
}

#[utoipa::path(
    get,
    path = "/api/v1/events/{environment}/{deployment_id}",
    responses(
        (status = 200, description = "Get events", body = serde_json::Value)
    ),
    params(
        ("deployment_id" = str, Path, description = "Deployment id that you want to see"),
        ("environment" = str, Path, description = "Environment of the deployment")
    ),
    description = "Describe Events"
)]
async fn get_events(
    Path((environment, deployment_id)): Path<(String, String)>,
) -> impl IntoResponse {
    let handler = env_common::AwsHandler {}; // Temporary, will be replaced with get_handler()

    let events = match handler.get_events(&deployment_id).await {
        Ok(events) => events,
        Err(e) => {
            let error_json = json!({"error": format!("{:?}", e)});
            return (StatusCode::NOT_FOUND, Json(error_json)).into_response();
        }
    };

    Json(events).into_response()
}

#[utoipa::path(
    get,
    path = "/api/v1/modules",
    responses(
        (status = 200, description = "Get ModulesPayload", body = Vec<ModuleV1>)
    ),
    description = "Get modules"
)]
#[debug_handler]
async fn get_modules() -> axum::Json<Vec<ModuleV1>> {
    let environment = "dev".to_string();

    let handler = env_common::AwsHandler {}; // Temporary, will be replaced with get_handler()

    let modules = match handler.list_module(&environment).await {
        Ok(modules) => modules,
        Err(e) => {
            let empty: Vec<env_defs::ModuleResp> = vec![];
            empty
        }
    };

    let result: Vec<ModuleV1> = modules
        .iter()
        .map(|module| ModuleV1 {
            environment: module.environment.clone(),
            environment_version: module.environment_version.clone(),
            version: module.version.clone(),
            timestamp: module.timestamp.clone(),
            module_name: module.module_name.clone(),
            module: module.module.clone(),
            description: module.description.clone(),
            reference: module.reference.clone(),
            manifest: module.manifest.clone(),
            tf_variables: module.tf_variables.clone(),
            tf_outputs: module.tf_outputs.clone(),
            s3_key: module.s3_key.clone(),
        })
        .collect();
    axum::Json(result)
}


#[utoipa::path(
    get,
    path = "/api/v1/policies",
    responses(
        (status = 200, description = "Get PoliciesPayload", body = Vec<PolicyV1>)
    ),
    description = "Get policies"
)]
#[debug_handler]
async fn get_policies() -> axum::Json<Vec<PolicyV1>> {
    let environment = "dev".to_string();

    let handler = env_common::AwsHandler {}; // Temporary, will be replaced with get_handler()

    let policies = match handler.list_policy(&environment).await {
        Ok(policies) => policies,
        Err(e) => {
            let empty: Vec<env_defs::PolicyResp> = vec![];
            empty
        }
    };

    let result: Vec<PolicyV1> = policies
        .iter()
        .map(|policy| PolicyV1 {
            environment: policy.environment.clone(),
            environment_version: policy.environment_version.clone(),
            version: policy.version.clone(),
            timestamp: policy.timestamp.clone(),
            policy_name: policy.policy_name.clone(),
            policy: policy.policy.clone(),
            description: policy.description.clone(),
            reference: policy.reference.clone(),
            manifest: policy.manifest.clone(),
            s3_key: policy.s3_key.clone(),
            data: policy.data.clone(),
        })
        .collect();
    axum::Json(result)
}

#[utoipa::path(
    get,
    path = "/api/v1/deployments",
    responses(
        (status = 200, description = "Get Deployments", body = Vec<DeploymentV1>)
    ),
    description = "Get deployments"
)]
#[debug_handler]
async fn get_deployments() -> axum::Json<Vec<DeploymentV1>> {
    let handler = env_common::AwsHandler {}; // Temporary, will be replaced with get_handler()

    let deployments = match handler.list_deployments().await {
        Ok(modules) => modules,
        Err(e) => {
            let empty: Vec<env_defs::DeploymentResp> = vec![];
            empty
        }
    };

    let result: Vec<DeploymentV1> = deployments
        .iter()
        .map(|deployment| DeploymentV1 {
            environment: deployment.environment.clone(),
            epoch: deployment.epoch.clone(),
            deployment_id: deployment.deployment_id.clone(),
            status: deployment.status.clone(),
            module: deployment.module.clone(),
            module_version: deployment.module_version.clone(),
            variables: deployment.variables.clone(),
            output: deployment.output.clone(),
            policy_results: deployment.policy_results.clone(),
            error_text: deployment.error_text.clone(),
            deleted: deployment.deleted.clone(),
            dependencies: deployment.dependencies.iter().map(|d| {
                DependencyV1 {
                    deployment_id: d.deployment_id.clone(),
                    environment: d.environment.clone(),
                }}).collect(),
            dependants: vec![], // Would require a separate call, maybe not necessary to have
        })
        .collect();
    axum::Json(result)
}

#[utoipa::path(
    get,
    path = "/api/v1/event_data",
    responses(
        (status = 200, description = "Get EventData", body = EventData)
    ),
    description = r#"Get event data for a deployment.

## Description
This will show the number of events that have occurred for a deployment."#,
)]
async fn get_event_data() -> axum::Json<Vec<EventData>> {
    let event_data = EventData {
        deployment_id: "deploy123".to_string(),
        event: "build_success".to_string(),
        epoch: 1627596000,
        error_text: "".to_string(),
        id: "event123".to_string(),
        job_id: "job456".to_string(),
        metadata: json!({"meta": "data"}),
        module: "backend".to_string(),
        name: "BuildBackend".to_string(),
        status: "success".to_string(),
        timestamp: "2023-09-20T12:34:56Z".to_string(),
    };
    axum::Json(vec![event_data; 100])
}

fn get_handler() -> Box<dyn env_common::ModuleEnvironmentHandler> {
    let cloud = "aws";
    let cloud_handler: Box<dyn env_common::ModuleEnvironmentHandler> = match cloud {
        "azure" => Box::new(env_common::AzureHandler {}),
        "aws" => Box::new(env_common::AwsHandler {}),
        _ => panic!("Invalid cloud provider"),
    };
    return cloud_handler;
}
