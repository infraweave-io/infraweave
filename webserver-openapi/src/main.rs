mod structs;
use axum::extract::Path;
use axum::response::IntoResponse;
use axum::{Json, Router};

use axum_macros::debug_handler;

use env_common::interface::{initialize_project_id, CloudHandler};
use env_common::logic::{workload_handler, handler};
use env_defs::{ModuleResp, ProjectData};
use env_utils::setup_logging;
use hyper::StatusCode;
use serde_json::json;
use std::io::Error;
use std::net::{Ipv4Addr, SocketAddr};
use structs::{DependantsV1, DependencyV1, DeploymentV1, EventData, ModuleV1, PolicyV1, ProjectDataV1};
use tokio::net::TcpListener;
use utoipa::{
    Modify, OpenApi,
};
use utoipa_rapidoc::RapiDoc;
use utoipa_redoc::{Redoc, Servable};
use utoipa_scalar::{Scalar, Servable as ScalarServable};
use utoipa_swagger_ui::SwaggerUi;

#[derive(OpenApi)]
#[openapi(
    paths(describe_deployment, get_event_data, get_modules, get_projects, get_deployments, read_logs, get_policies, get_policy_version, get_module_version, get_deployments_for_module, get_events, get_all_versions_for_module, get_stacks, get_stack_version, get_change_record),
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
    initialize_project_id().await;
    setup_logging(log::LevelFilter::Info).unwrap();
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
            "/api/v1/stack/:stack_name/:stack_version",
            axum::routing::get(get_stack_version),
        )
        .route(
            "/api/v1/policy/:policy_name/:policy_version",
            axum::routing::get(get_policy_version),
        )
        .route(
            "/api/v1/deployment/:project/:region/:environment/:deployment_id",
            axum::routing::get(describe_deployment),
        )
        .route(
            "/api/v1/deployments/module/:project/:region/:module",
            axum::routing::get(get_deployments_for_module),
        )
        .route(
            "/api/v1/logs/:project/:region/:environment/:deployment_id",
            axum::routing::get(read_logs),
        )
        .route(
            "/api/v1/events/:project/:region/:environment/:deployment_id",
            axum::routing::get(get_events),
        )
        .route(
            "/api/v1/change_record/:project/:region/:environment/:deployment_id/:job_id/:change_type",
            axum::routing::get(get_change_record),
        )
        .route(
            "/api/v1/modules/versions/:module",
            axum::routing::get(get_all_versions_for_module),
        )
        .route(
            "/api/v1/stacks/versions/:stack",
            axum::routing::get(get_all_versions_for_stack),
        )
        .route("/api/v1/modules", axum::routing::get(get_modules))
        .route("/api/v1/projects", axum::routing::get(get_projects))
        .route("/api/v1/stacks", axum::routing::get(get_stacks))
        .route("/api/v1/policies", axum::routing::get(get_policies))
        .route("/api/v1/deployments/:project/:region", axum::routing::get(get_deployments))
        .route("/api/v1/event_data/:project", axum::routing::get(get_event_data));

    let address = SocketAddr::from((Ipv4Addr::UNSPECIFIED, 8081));
    let listener = TcpListener::bind(&address).await?;
    axum::serve(listener, app.into_make_service()).await
}

#[utoipa::path(
    get,
    path = "/api/v1/deployment/{project}/{region}/{environment}/{deployment_id}",
    responses(
        (status = 200, description = "Get DeploymentV1", body = Option<DeploymentV1, serde_json::Value>)
    ),
    params(
        ("project" = str, Path, description = "Project id that you want to see"),
        ("region" = str, Path, description = "Region that you want to see"),
        ("deployment_id" = str, Path, description = "Deployment id that you want to see"),
        ("environment" = str, Path, description = "Environment of the deployment")
    ),
    description = "Describe DeploymentV1"
)]
async fn describe_deployment(
    Path((project, region, environment, deployment_id)): Path<(String, String, String, String)>,
) -> impl IntoResponse {
    let (deployment, dependents) = match workload_handler(&project, &region).get_deployment_and_dependents(&deployment_id, &environment, false).await
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

    let deployment_v1 = DeploymentV1 {
        environment: deployment.environment.clone(),
        epoch: deployment.epoch.clone(),
        deployment_id: deployment.deployment_id.clone(),
        status: deployment.status.clone(),
        job_id: deployment.job_id.clone(),
        module: deployment.module.clone(),
        module_version: deployment.module_version.clone(),
        module_type: deployment.module_type.clone(),
        drift_detection: deployment.drift_detection.clone(),
        next_drift_check_epoch: deployment.next_drift_check_epoch.clone(),
        has_drifted: deployment.has_drifted.clone(),
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
        initiated_by: deployment.initiated_by.clone(),
    };

    (StatusCode::OK, Json(deployment_v1)).into_response()
}


#[utoipa::path(
    get,
    path = "/api/v1/stack/{stack_name}/{stack_version}",
    responses(
        (status = 200, description = "Get stack", body = Option<ModuleV1, serde_json::Value>)
    ),
    params(
        ("stack_name" = str, Path, description = "Stack name that you want to see"),
        ("stack_version" = str, Path, description = "Stack version that you want to see"),
    ),
    description = "Get stack version"
)]
async fn get_stack_version(
    Path((stack_name, stack_version)): Path<(String, String)>,
) -> impl IntoResponse {

    let track = "dev".to_string(); // TODO: Get from request

    let stack = match handler().get_stack_version(&stack_name, &track, &stack_version)
        .await
    {
        Ok(result) => {
            match result {
                Some(stack) => stack,
                None => {
                    let error_json = json!({"error": "Stack not found"});
                    return (StatusCode::NOT_FOUND, Json(error_json)).into_response();
                }
            }
        },
        Err(e) => {
            let error_json = json!({"error": format!("{:?}", e)});
            return (StatusCode::NOT_FOUND, Json(error_json)).into_response();
        }
    };

    (StatusCode::OK, Json(parse_module(&stack))).into_response()
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

    let track = "dev".to_string(); // TODO: Get from request

    let module = match handler().get_module_version(&module_name, &track, &module_version)
        .await
    {
        Ok(module) => {
            match module {
                Some(module) => module,
                None => {
                    let error_json = json!({"error": "Module not found"});
                    return (StatusCode::NOT_FOUND, Json(error_json)).into_response();
                }
            }
        },
        Err(e) => {
            let error_json = json!({"error": format!("{:?}", e)});
            return (StatusCode::NOT_FOUND, Json(error_json)).into_response();
        }
    };

    let response = parse_module(&module);

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
    let environment = "dev".to_string();// TODO: Get from request

    let policy = match handler().get_policy(&policy_name, &environment, &policy_version)
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
    path = "/api/v1/logs/{project}/{region}/{environment}/{job_id}",
    responses(
        (status = 200, description = "Get logs", body = serde_json::Value)
    ),
    params(
        ("job_id" = str, Path, description = "Job id that you want to see"),
        ("region" = str, Path, description = "Region that you want to see"),
        ("environment" = str, Path, description = "Environment of the deployment"),
        ("project" = str, Path, description = "Project id that you want to see"),
    ),
    description = "Describe DeploymentV1"
)]
async fn read_logs(
    Path((project, region, environment, job_id)): Path<(String, String, String, String)>,
) -> impl IntoResponse {
    let log_str = match workload_handler(&project, &region).read_logs(&job_id).await {
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

    let events = match workload_handler(&project, &region).get_events(&deployment_id, &environment).await {
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
    Path((project, region, environment, deployment_id, job_id, change_type)): Path<(String, String, String, String, String, String)>,
) -> impl IntoResponse {

    let events = match workload_handler(&project, &region).get_change_record(&environment, &deployment_id, &job_id, &change_type).await {
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
    let track = "dev".to_string();

    let modules = match handler().get_all_latest_module(&track).await {
        Ok(modules) => modules,
        Err(e) => {
            let empty: Vec<env_defs::ModuleResp> = vec![];
            empty
        }
    };

    let result: Vec<ModuleV1> = modules
        .iter()
        .map(|module| parse_module(module))
        .collect();
    axum::Json(result)
}


#[utoipa::path(
    get,
    path = "/api/v1/projects",
    responses(
        (status = 200, description = "Get all projects", body = Vec<ProjectDataV1>)
    ),
    description = "Get all projects"
)]
#[debug_handler]
async fn get_projects() -> axum::Json<Vec<ProjectDataV1>> {
    let projects = match handler().get_all_projects().await {
        Ok(projects) => projects,
        Err(e) => {
            let empty: Vec<ProjectData> = vec![];
            empty
        }
    };

    let result: Vec<ProjectDataV1> = projects
        .iter()
        .map(|project| parse_project(project))
        .collect();
    axum::Json(result)
}


#[utoipa::path(
    get,
    path = "/api/v1/stacks",
    responses(
        (status = 200, description = "Get ModulesPayload", body = Vec<ModuleV1>)
    ),
    description = "Get stacks"
)]
#[debug_handler]
async fn get_stacks() -> axum::Json<Vec<ModuleV1>> {
    let track = "dev".to_string();

    let modules = match handler().get_all_latest_stack(&track).await {
        Ok(modules) => modules,
        Err(e) => {
            let empty: Vec<env_defs::ModuleResp> = vec![];
            empty
        }
    };

    let result: Vec<ModuleV1> = modules
        .iter()
        .map(|module| parse_module(module))
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

    let policies = match handler().get_all_policies(&environment).await {
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
    path = "/api/v1/modules/versions/{module}",
    responses(
        (status = 200, description = "Get versions for module", body = Vec<ModuleV1>)
    ),
    description = "Get versions for module",
    params(
        ("module" = str, Path, description = "Module name that you want to see"),
    ),
)]
#[debug_handler]
async fn get_all_versions_for_module(
    Path(module): Path<String>,
) -> axum::Json<Vec<ModuleV1>> {

    let track = "dev".to_string();
    let modules = match handler().get_all_module_versions(&module, &track).await {
        Ok(modules) => modules,
        Err(e) => {
            let empty: Vec<env_defs::ModuleResp> = vec![];
            empty
        }
    };

    let result: Vec<ModuleV1> = modules
        .iter()
        .map(|module| parse_module(module))
        .collect();
    axum::Json(result)
}

#[utoipa::path(
    get,
    path = "/api/v1/stacks/versions/{stack}",
    responses(
        (status = 200, description = "Get versions for stack", body = Vec<ModuleV1>)
    ),
    description = "Get versions for stack",
    params(
        ("stack" = str, Path, description = "Stack name that you want to see"),
    ),
)]
#[debug_handler]
async fn get_all_versions_for_stack(
    Path(stack): Path<String>,
) -> axum::Json<Vec<ModuleV1>> {

    let track = "dev".to_string();
    let modules = match handler().get_all_stack_versions(&stack, &track).await {
        Ok(modules) => modules,
        Err(e) => {
            let empty: Vec<env_defs::ModuleResp> = vec![];
            empty
        }
    };

    let result: Vec<ModuleV1> = modules
        .iter()
        .map(|module| parse_module(module))
        .collect();
    axum::Json(result)
}

#[utoipa::path(
    get,
    path = "/api/v1/deployments/module/{project}/{region}/{module}",
    responses(
        (status = 200, description = "Get Deployments", body = Vec<DeploymentV1>)
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
) -> axum::Json<Vec<DeploymentV1>> {
    let environment = ""; // this can be used to filter out specific environments
    let deployments = match workload_handler(&project, &region).get_deployments_using_module(&module, &environment).await {
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
            job_id: deployment.job_id.clone(),
            module: deployment.module.clone(),
            module_version: deployment.module_version.clone(),
            module_type: deployment.module_type.clone(),
            drift_detection: deployment.drift_detection.clone(),
            next_drift_check_epoch: deployment.next_drift_check_epoch.clone(),
            has_drifted: deployment.has_drifted.clone(),
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
            initiated_by: deployment.initiated_by.clone(),
        })
        .collect();
    axum::Json(result)
}

#[utoipa::path(
    get,
    path = "/api/v1/deployments/{project}/{region}",
    responses(
        (status = 200, description = "Get Deployments", body = Vec<DeploymentV1>)
    ),
    description = "Get deployments",
    params(
        ("project" = str, Path, description = "Project id that you want to see"),
        ("region" = str, Path, description = "Region that you want to see"),
    ),
)]
#[debug_handler]
async fn get_deployments(
    Path((project, region)): Path<(String, String)>,
) -> axum::Json<Vec<DeploymentV1>> {
    let deployments = match workload_handler(&project, &region).get_all_deployments("").await {
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
            job_id: deployment.job_id.clone(),
            module: deployment.module.clone(),
            module_version: deployment.module_version.clone(),
            module_type: deployment.module_type.clone(),
            drift_detection: deployment.drift_detection.clone(),
            next_drift_check_epoch: deployment.next_drift_check_epoch.clone(),
            has_drifted: deployment.has_drifted.clone(),
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
            initiated_by: deployment.initiated_by.clone(),
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

fn parse_module(module: &ModuleResp) -> ModuleV1 {
    ModuleV1 {
        track: module.track.clone(),
        track_version: module.track_version.clone(),
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
        stack_data: module.stack_data.clone(),
        version_diff: module.version_diff.clone(),
    }
}

fn parse_project(project: &ProjectData) -> ProjectDataV1 {
    ProjectDataV1 {
        project_id: project.project_id.clone(),
        description: project.description.clone(),
        name: project.name.clone(),
        regions: project.regions.clone(),
    }
}