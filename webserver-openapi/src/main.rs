use axum::extract::Path;
use axum::response::IntoResponse;
use axum::{Json, Router};
use axum_macros::debug_handler;
use log::error;

use env_common::interface::{
    get_current_identity, initialize_project_id_and_region, GenericCloudHandler,
};
use env_defs::{
    CloudProvider, CloudProviderCommon, Dependency, Dependent, DeploymentResp, ModuleResp,
    PolicyResp, ProjectData,
};
use env_utils::setup_logging;
use hyper::StatusCode;
use serde_json::json;
use std::io::Error;
use std::net::{Ipv4Addr, SocketAddr};
use tokio::net::TcpListener;
use utoipa::{Modify, OpenApi};
use utoipa_redoc::{Redoc, Servable};
use utoipa_swagger_ui::SwaggerUi;

#[derive(OpenApi)]
#[openapi(
    paths(describe_deployment, get_modules, get_projects, get_deployments, read_logs, get_policies, get_policy_version, get_module_version, get_deployments_for_module, get_events, get_all_versions_for_module, get_stacks, get_stack_version, get_change_record),
    components(schemas(ModuleResp, DeploymentResp, PolicyResp, Dependency, Dependent, ProjectData)),
    modifiers(&SecurityAddon),
    tags(
        (name = "api", description = "API for custom structs")
    )
)]
struct ApiDoc;

struct SecurityAddon;

impl Modify for SecurityAddon {
    fn modify(&self, _openapi: &mut utoipa::openapi::OpenApi) {
        // if let Some(components) = openapi.components.as_mut() {
        // components.add_security_scheme(
        //     "api_key",
        //     SecurityScheme::ApiKey(ApiKey::Header(ApiKeyValue::new("todo_apikey"))),
        // )
        // }
    }
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    initialize_project_id_and_region().await;
    setup_logging().unwrap();
    // get_current_identity().await;
    let app = Router::new()
        .merge(SwaggerUi::new("/swagger-ui").url("/api-docs/openapi.json", ApiDoc::openapi()))
        .merge(Redoc::with_url("/redoc", ApiDoc::openapi()))
        .route(
            "/api/v1/module/{track}/{module_name}/{module_version}",
            axum::routing::get(get_module_version),
        )
        .route(
            "/api/v1/stack/{track}/{stack_name}/{stack_version}",
            axum::routing::get(get_stack_version),
        )
        .route(
            "/api/v1/policy/{environment}/{policy_name}/{policy_version}",
            axum::routing::get(get_policy_version),
        )
        .route(
            "/api/v1/deployment/{project}/{region}/{environment}/{deployment_id}",
            axum::routing::get(describe_deployment),
        )
        .route(
            "/api/v1/deployments/module/{project}/{region}/{module}",
            axum::routing::get(get_deployments_for_module),
        )
        .route(
            "/api/v1/logs/{project}/{region}/{job_id}",
            axum::routing::get(read_logs),
        )
        .route(
            "/api/v1/events/{project}/{region}/{environment}/{deployment_id}",
            axum::routing::get(get_events),
        )
        .route(
            "/api/v1/change_record/{project}/{region}/{environment}/{deployment_id}/{job_id}/{change_type}",
            axum::routing::get(get_change_record),
        )
        .route(
            "/api/v1/modules/versions/{track}/{module}",
            axum::routing::get(get_all_versions_for_module),
        )
        .route(
            "/api/v1/stacks/versions/{track}/{stack}",
            axum::routing::get(get_all_versions_for_stack),
        )
        .route("/api/v1/modules", axum::routing::get(get_modules))
        .route("/api/v1/projects", axum::routing::get(get_projects))
        .route("/api/v1/stacks", axum::routing::get(get_stacks))
        .route("/api/v1/policies/{environment}", axum::routing::get(get_policies))
        .route("/api/v1/deployments/{project}/{region}", axum::routing::get(get_deployments));

    let address = SocketAddr::from((Ipv4Addr::UNSPECIFIED, 8081));
    let listener = TcpListener::bind(&address).await?;
    axum::serve(listener, app.into_make_service()).await
}

#[utoipa::path(
    get,
    path = "/api/v1/deployment/{project}/{region}/{environment}/{deployment_id}",
    responses(
        (status = 200, description = "Get DeploymentResp", body = Option<DeploymentResp, serde_json::Value>)
    ),
    params(
        ("project" = str, Path, description = "Project id that you want to see"),
        ("region" = str, Path, description = "Region that you want to see"),
        ("deployment_id" = str, Path, description = "Deployment id that you want to see"),
        ("environment" = str, Path, description = "Environment of the deployment")
    ),
    description = "Describe DeploymentResp"
)]
async fn describe_deployment(
    Path((project, region, environment, deployment_id)): Path<(String, String, String, String)>,
) -> impl IntoResponse {
    let (deployment, _dependents) = match GenericCloudHandler::workload(&project, &region)
        .await
        .get_deployment_and_dependents(&deployment_id, &environment, false)
        .await
    {
        Ok((deployment, dependents)) => match deployment {
            Some(deployment) => (deployment, dependents),
            None => {
                let error_json = json!({"error": "Deployment not found"});
                return (StatusCode::NOT_FOUND, Json(error_json)).into_response();
            }
        },
        Err(e) => {
            let error_json = json!({"error": format!("{:?}", e)});
            return (StatusCode::NOT_FOUND, Json(error_json)).into_response();
        }
    };

    (StatusCode::OK, Json(deployment)).into_response()
}

#[utoipa::path(
    get,
    path = "/api/v1/stack/${track}/{stack_name}/{stack_version}",
    responses(
        (status = 200, description = "Get stack", body = Option<ModuleResp, serde_json::Value>)
    ),
    params(
        ("track" = str, Path, description = "Track that you want to see"),
        ("stack_name" = str, Path, description = "Stack name that you want to see"),
        ("stack_version" = str, Path, description = "Stack version that you want to see"),
    ),
    description = "Get stack version"
)]
async fn get_stack_version(
    Path((track, stack_name, stack_version)): Path<(String, String, String)>,
) -> impl IntoResponse {
    let stack = match GenericCloudHandler::default()
        .await
        .get_stack_version(&stack_name, &track, &stack_version)
        .await
    {
        Ok(result) => match result {
            Some(stack) => stack,
            None => {
                let error_json = json!({"error": "Stack not found"});
                return (StatusCode::NOT_FOUND, Json(error_json)).into_response();
            }
        },
        Err(e) => {
            let error_json = json!({"error": format!("{:?}", e)});
            return (StatusCode::NOT_FOUND, Json(error_json)).into_response();
        }
    };

    (StatusCode::OK, Json(stack)).into_response()
}

#[utoipa::path(
    get,
    path = "/api/v1/module/{track}/{module_name}/{module_version}",
    responses(
        (status = 200, description = "Get module", body = Option<ModuleResp, serde_json::Value>)
    ),
    params(
        ("track" = str, Path, description = "Track that you want to see"),
        ("module_name" = str, Path, description = "Module name that you want to see"),
        ("module_version" = str, Path, description = "Module version that you want to see"),
    ),
    description = "Get module version"
)]
async fn get_module_version(
    Path((track, module_name, module_version)): Path<(String, String, String)>,
) -> impl IntoResponse {
    let module = match GenericCloudHandler::default()
        .await
        .get_module_version(&module_name, &track, &module_version)
        .await
    {
        Ok(module) => match module {
            Some(module) => module,
            None => {
                let error_json = json!({"error": "Module not found"});
                return (StatusCode::NOT_FOUND, Json(error_json)).into_response();
            }
        },
        Err(e) => {
            let error_json = json!({"error": format!("{:?}", e)});
            return (StatusCode::NOT_FOUND, Json(error_json)).into_response();
        }
    };

    (StatusCode::OK, Json(module)).into_response()
}

#[utoipa::path(
    get,
    path = "/api/v1/policy/{environment}/{policy_name}/{policy_version}",
    responses(
        (status = 200, description = "Get policy", body = Option<PolicyResp, serde_json::Value>)
    ),
    params(
        ("environment" = str, Path, description = "Environment that you want to see"),
        ("policy_name" = str, Path, description = "Policy name that you want to see"),
        ("policy_version" = str, Path, description = "Policy version that you want to see"),
    ),
    description = "Get policy version"
)]
async fn get_policy_version(
    Path((environment, policy_name, policy_version)): Path<(String, String, String)>,
) -> impl IntoResponse {
    let policy = match GenericCloudHandler::default()
        .await
        .get_policy(&policy_name, &environment, &policy_version)
        .await
    {
        Ok(policy) => policy,
        Err(e) => {
            let error_json = json!({"error": format!("{:?}", e)});
            return (StatusCode::NOT_FOUND, Json(error_json)).into_response();
        }
    };

    let response = PolicyResp {
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
    path = "/api/v1/logs/{project}/{region}/{job_id}",
    responses(
        (status = 200, description = "Get logs", body = serde_json::Value)
    ),
    params(
        ("job_id" = str, Path, description = "Job id that you want to see"),
        ("region" = str, Path, description = "Region that you want to see"),
        ("project" = str, Path, description = "Project id that you want to see"),
    ),
    description = "Describe DeploymentResp"
)]
async fn read_logs(
    Path((project, region, job_id)): Path<(String, String, String)>,
) -> impl IntoResponse {
    let log_str = match GenericCloudHandler::workload(&project, &region)
        .await
        .read_logs(&job_id)
        .await
    {
        Ok(logs) => {
            let mut log_str = String::new();
            for log in logs {
                log_str.push_str(&format!("{}\n", log.message));
            }
            log_str
        }
        Err(e) => {
            let error_json = json!({"error": format!("{:?}", e)});
            return (StatusCode::NOT_FOUND, Json(error_json)).into_response();
        }
    };

    Json(json!({"logs": log_str})).into_response()
}

#[utoipa::path(
    get,
    path = "/api/v1/events/{project}/{region}/{environment}/{deployment_id}",
    responses(
        (status = 200, description = "Get events", body = serde_json::Value)
    ),
    params(
        ("project" = str, Path, description = "Project id that you want to see"),
        ("region" = str, Path, description = "Region that you want to see"),
        ("environment" = str, Path, description = "Environment of the deployment"),
        ("deployment_id" = str, Path, description = "Deployment id that you want to see"),
    ),
    description = "Describe Events"
)]
async fn get_events(
    Path((project, region, environment, deployment_id)): Path<(String, String, String, String)>,
) -> impl IntoResponse {
    let events = match GenericCloudHandler::workload(&project, &region)
        .await
        .get_events(&deployment_id, &environment)
        .await
    {
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
    path = "/api/v1/change_record/{project}/{region}/{environment}/{deployment_id}/{job_id}/{change_type}",
    responses(
        (status = 200, description = "Get change record", body = serde_json::Value)
    ),
    params(
        ("project" = str, Path, description = "Project id that you want to see"),
        ("region" = str, Path, description = "Region that you want to see"),
        ("environment" = str, Path, description = "Environment of the deployment"),
        ("deployment_id" = str, Path, description = "deployment id that you want to see"),
        ("job_id" = str, Path, description = "job id that you want to see"),
        ("change_type" = str, Path, description = "job id that you want to see"),
    ),
    description = "Describe change record"
)]
async fn get_change_record(
    Path((project, region, environment, deployment_id, job_id, change_type)): Path<(
        String,
        String,
        String,
        String,
        String,
        String,
    )>,
) -> impl IntoResponse {
    let change_record = match GenericCloudHandler::workload(&project, &region)
        .await
        .get_change_record(&environment, &deployment_id, &job_id, &change_type)
        .await
    {
        Ok(change_record) => change_record,
        Err(e) => {
            let error_json = json!({"error": format!("{:?}", e)});
            return (StatusCode::INTERNAL_SERVER_ERROR, Json(error_json)).into_response();
        }
    };

    Json(change_record).into_response()
}

#[utoipa::path(
    get,
    path = "/api/v1/modules",
    responses(
        (status = 200, description = "Get ModulesPayload", body = Vec<ModuleResp>)
    ),
    description = "Get modules"
)]
#[debug_handler]
async fn get_modules() -> impl IntoResponse {
    let track = "".to_string(); // Don't filter by track

    let modules = match GenericCloudHandler::default()
        .await
        .get_all_latest_module(&track)
        .await
    {
        Ok(modules) => modules,
        Err(_e) => {
            error!("Error get_deployments(): {:?}", _e);
            let empty: Vec<env_defs::ModuleResp> = vec![];
            empty
        }
    };
    Json(modules).into_response()
}

#[utoipa::path(
    get,
    path = "/api/v1/projects",
    responses(
        (status = 200, description = "Get all projects", body = Vec<ProjectData>)
    ),
    description = "Get all projects"
)]
#[debug_handler]
async fn get_projects() -> impl IntoResponse {
    let projects = match GenericCloudHandler::default()
        .await
        .get_all_projects()
        .await
    {
        Ok(projects) => projects,
        Err(_e) => {
            let error_json = json!({"error": format!("{:?}", _e)});
            return (StatusCode::INTERNAL_SERVER_ERROR, Json(error_json)).into_response();
        }
    };

    Json(projects).into_response()
}

#[utoipa::path(
    get,
    path = "/api/v1/stacks",
    responses(
        (status = 200, description = "Get ModulesPayload", body = Vec<ModuleResp>)
    ),
    description = "Get stacks"
)]
#[debug_handler]
async fn get_stacks() -> impl IntoResponse {
    let track = "".to_string(); // Don't filter by track

    let stacks = match GenericCloudHandler::default()
        .await
        .get_all_latest_stack(&track)
        .await
    {
        Ok(stack_modules) => stack_modules,
        Err(_e) => {
            let error_json = json!({"error": format!("{:?}", _e)});
            return (StatusCode::INTERNAL_SERVER_ERROR, Json(error_json)).into_response();
        }
    };
    Json(stacks).into_response()
}

#[utoipa::path(
    get,
    path = "/api/v1/policies/{environment}",
    responses(
        (status = 200, description = "Get PoliciesPayload", body = Vec<PolicyResp>)
    ),
    params(
        ("environment" = str, Path, description = "Environment that you want to see for a specific environment"),
    ),
    description = "Get policies"
)]
#[debug_handler]
async fn get_policies(Path(environment): Path<String>) -> impl IntoResponse {
    let policies = match GenericCloudHandler::default()
        .await
        .get_all_policies(&environment)
        .await
    {
        Ok(policies) => policies,
        Err(_e) => {
            let error_json = json!({"error": format!("{:?}", _e)});
            return (StatusCode::INTERNAL_SERVER_ERROR, Json(error_json)).into_response();
        }
    };
    Json(policies).into_response()
}

#[utoipa::path(
    get,
    path = "/api/v1/modules/versions/{track}/{module}",
    responses(
        (status = 200, description = "Get versions for module", body = Vec<ModuleResp>)
    ),
    description = "Get versions for module",
    params(
        ("track" = str, Path, description = "Track name that you want to see"),
        ("module" = str, Path, description = "Module name that you want to see"),
    ),
)]
#[debug_handler]
async fn get_all_versions_for_module(
    Path((track, module)): Path<(String, String)>,
) -> impl IntoResponse {
    let modules = match GenericCloudHandler::default()
        .await
        .get_all_module_versions(&module, &track)
        .await
    {
        Ok(modules) => modules,
        Err(_e) => {
            let error_json = json!({"error": format!("{:?}", _e)});
            return (StatusCode::INTERNAL_SERVER_ERROR, Json(error_json)).into_response();
        }
    };
    Json(modules).into_response()
}

#[utoipa::path(
    get,
    path = "/api/v1/stacks/versions/{track}/{stack}",
    responses(
        (status = 200, description = "Get versions for stack", body = Vec<ModuleResp>)
    ),
    description = "Get versions for stack",
    params(
        ("track" = str, Path, description = "Track name that you want to see"),
        ("stack" = str, Path, description = "Stack name that you want to see"),
    ),
)]
#[debug_handler]
async fn get_all_versions_for_stack(
    Path((track, stack)): Path<(String, String)>,
) -> impl IntoResponse {
    let modules = match GenericCloudHandler::default()
        .await
        .get_all_stack_versions(&stack, &track)
        .await
    {
        Ok(modules) => modules,
        Err(_e) => {
            let error_json = json!({"error": format!("{:?}", _e)});
            return (StatusCode::INTERNAL_SERVER_ERROR, Json(error_json)).into_response();
        }
    };
    Json(modules).into_response()
}

#[utoipa::path(
    get,
    path = "/api/v1/deployments/module/{project}/{region}/{module}",
    responses(
        (status = 200, description = "Get Deployments", body = Vec<DeploymentResp>)
    ),
    description = "Get deployments",
    params(
        ("project" = str, Path, description = "Project id that you want to see"),
        ("region" = str, Path, description = "Region that you want to see"),
        ("module" = str, Path, description = "Module name that you want to see"),
    ),
)]
#[debug_handler]
async fn get_deployments_for_module(
    Path((project, region, module)): Path<(String, String, String)>,
) -> impl IntoResponse {
    let environment = ""; // this can be used to filter out specific environments
    let deployments = match GenericCloudHandler::workload(&project, &region)
        .await
        .get_deployments_using_module(&module, environment)
        .await
    {
        Ok(modules) => modules,
        Err(_e) => {
            let error_json = json!({"error": format!("{:?}", _e)});
            return (StatusCode::INTERNAL_SERVER_ERROR, Json(error_json)).into_response();
        }
    };
    Json(deployments).into_response()
}

#[utoipa::path(
    get,
    path = "/api/v1/deployments/{project}/{region}",
    responses(
        (status = 200, description = "Get Deployments", body = Vec<DeploymentResp>)
    ),
    description = "Get deployments",
    params(
        ("project" = str, Path, description = "Project id that you want to see"),
        ("region" = str, Path, description = "Region that you want to see"),
    ),
)]
#[debug_handler]
async fn get_deployments(Path((project, region)): Path<(String, String)>) -> impl IntoResponse {
    let deployments = match GenericCloudHandler::workload(&project, &region)
        .await
        .get_all_deployments("")
        .await
    {
        Ok(deployments) => deployments,
        Err(_e) => {
            let error_json = json!({"error": format!("{:?}", _e)});
            return (StatusCode::INTERNAL_SERVER_ERROR, Json(error_json)).into_response();
        }
    };

    Json(deployments).into_response()
}
