use crate::auth::{self, project_access_middleware};
use crate::handlers;
use axum::{middleware, Router};
use env_common::interface::initialize_project_id_and_region;
use env_utils::setup_logging;
use std::io::Error;
use std::net::{Ipv4Addr, SocketAddr};
use tokio::net::TcpListener;
use tower_http::trace::TraceLayer;

#[cfg(feature = "ui")]
use utoipa_redoc::{Redoc, Servable};
#[cfg(feature = "ui")]
use utoipa_swagger_ui::SwaggerUi;

pub async fn run_server() -> Result<(), Error> {
    run_server_on_port(8081, true, false).await
}

#[cfg_attr(not(feature = "ui"), allow(unused_variables))]
pub async fn run_server_with_listener(
    listener: TcpListener,
    enable_ui: bool,
    disable_auth: bool,
) -> Result<(), Error> {
    initialize_project_id_and_region().await;
    setup_logging().unwrap();

    // Disable JWT auth if requested (for MCP internal use)
    if disable_auth {
        std::env::set_var("DISABLE_JWT_AUTH_INSECURE", "true");
    }

    // Validate authentication configuration at startup
    let auth_warnings = auth::validate_auth_config();
    for warning in &auth_warnings {
        log::warn!("Auth config: {}", warning);
    }

    let protected_routes = Router::new()
        // All routes use JWT authentication with project access validation
        .route(
            "/api/v1/deployment/{project}/{region}/{environment}/{deployment_id}",
            axum::routing::get(handlers::describe_deployment),
        )
        .route(
            "/api/v1/deployments/module/{project}/{region}/{module}",
            axum::routing::get(handlers::get_deployments_for_module),
        )
        .route(
            "/api/v1/logs/{project}/{region}/{job_id}",
            axum::routing::get(handlers::read_logs),
        )
        .route(
            "/api/v1/events/{project}/{region}/{environment}/{deployment_id}",
            axum::routing::get(handlers::get_events),
        )
        .route(
            "/api/v1/change_record/{project}/{region}/{environment}/{deployment_id}/{job_id}/{change_type}",
            axum::routing::get(handlers::get_change_record),
        )
        .route(
            "/api/v1/deployments/{project}/{region}", 
            axum::routing::get(handlers::get_deployments)
        )
        .route(
            "/api/v1/module/{track}/{module_name}/{module_version}",
            axum::routing::get(handlers::get_module_version),
        )
        .route(
            "/api/v1/stack/{track}/{stack_name}/{stack_version}",
            axum::routing::get(handlers::get_stack_version),
        )
        .route(
            "/api/v1/policy/{environment}/{policy_name}/{policy_version}",
            axum::routing::get(handlers::get_policy_version),
        )
        .route(
            "/api/v1/modules/versions/{track}/{module}",
            axum::routing::get(handlers::get_all_versions_for_module),
        )
        .route(
            "/api/v1/stacks/versions/{track}/{stack}",
            axum::routing::get(handlers::get_all_versions_for_stack),
        )
        .route("/api/v1/modules", axum::routing::get(handlers::get_modules))
        .route("/api/v1/projects", axum::routing::get(handlers::get_projects))
        .route("/api/v1/stacks", axum::routing::get(handlers::get_stacks))
        .route("/api/v1/policies/{environment}", axum::routing::get(handlers::get_policies))
        // Single JWT-based authentication middleware
        .layer(middleware::from_fn(project_access_middleware))
        .layer(TraceLayer::new_for_http());

    let mut app = Router::new();

    // Only include Swagger UI and ReDoc if enabled
    #[cfg(feature = "ui")]
    if enable_ui {
        app = app
            .merge(
                SwaggerUi::new("/swagger-ui")
                    .url("/api-docs/openapi.json", handlers::ApiDoc::openapi()),
            )
            .merge(Redoc::with_url("/redoc", handlers::ApiDoc::openapi()));
    }

    app = app.merge(protected_routes);

    log::info!(
        "Starting web server on {} ({})",
        listener.local_addr().unwrap(),
        if disable_auth {
            "localhost only - MCP mode"
        } else {
            "network accessible - production mode"
        }
    );

    axum::serve(listener, app.into_make_service()).await
}

#[cfg_attr(not(feature = "ui"), allow(unused_variables))]
pub async fn run_server_on_port(
    port: u16,
    enable_ui: bool,
    disable_auth: bool,
) -> Result<(), Error> {
    initialize_project_id_and_region().await;
    setup_logging().unwrap();

    // Disable JWT auth if requested (for MCP internal use)
    if disable_auth {
        std::env::set_var("DISABLE_JWT_AUTH_INSECURE", "true");
    }

    // Validate authentication configuration at startup
    let auth_warnings = auth::validate_auth_config();
    for warning in &auth_warnings {
        log::warn!("Auth config: {}", warning);
    }

    let protected_routes = Router::new()
        // All routes use JWT authentication with project access validation
        .route(
            "/api/v1/deployment/{project}/{region}/{environment}/{deployment_id}",
            axum::routing::get(handlers::describe_deployment),
        )
        .route(
            "/api/v1/deployments/module/{project}/{region}/{module}",
            axum::routing::get(handlers::get_deployments_for_module),
        )
        .route(
            "/api/v1/logs/{project}/{region}/{job_id}",
            axum::routing::get(handlers::read_logs),
        )
        .route(
            "/api/v1/events/{project}/{region}/{environment}/{deployment_id}",
            axum::routing::get(handlers::get_events),
        )
        .route(
            "/api/v1/change_record/{project}/{region}/{environment}/{deployment_id}/{job_id}/{change_type}",
            axum::routing::get(handlers::get_change_record),
        )
        .route(
            "/api/v1/deployments/{project}/{region}", 
            axum::routing::get(handlers::get_deployments)
        )
        .route(
            "/api/v1/module/{track}/{module_name}/{module_version}",
            axum::routing::get(handlers::get_module_version),
        )
        .route(
            "/api/v1/stack/{track}/{stack_name}/{stack_version}",
            axum::routing::get(handlers::get_stack_version),
        )
        .route(
            "/api/v1/policy/{environment}/{policy_name}/{policy_version}",
            axum::routing::get(handlers::get_policy_version),
        )
        .route(
            "/api/v1/modules/versions/{track}/{module}",
            axum::routing::get(handlers::get_all_versions_for_module),
        )
        .route(
            "/api/v1/stacks/versions/{track}/{stack}",
            axum::routing::get(handlers::get_all_versions_for_stack),
        )
        .route("/api/v1/modules", axum::routing::get(handlers::get_modules))
        .route("/api/v1/projects", axum::routing::get(handlers::get_projects))
        .route("/api/v1/stacks", axum::routing::get(handlers::get_stacks))
        .route("/api/v1/policies/{environment}", axum::routing::get(handlers::get_policies))
        // Single JWT-based authentication middleware
        .layer(middleware::from_fn(project_access_middleware))
        .layer(TraceLayer::new_for_http());

    let mut app = Router::new();

    // Only include Swagger UI and ReDoc if enabled
    #[cfg(feature = "ui")]
    if enable_ui {
        app = app
            .merge(
                SwaggerUi::new("/swagger-ui")
                    .url("/api-docs/openapi.json", handlers::ApiDoc::openapi()),
            )
            .merge(Redoc::with_url("/redoc", handlers::ApiDoc::openapi()));
    }

    app = app.merge(protected_routes);

    // Bind to localhost only when auth is disabled (MCP mode)
    // Bind to 0.0.0.0 when auth is enabled (production mode)
    let bind_addr = if disable_auth {
        Ipv4Addr::LOCALHOST // 127.0.0.1 - only accessible locally
    } else {
        Ipv4Addr::UNSPECIFIED // 0.0.0.0 - accessible from network
    };

    let address = SocketAddr::from((bind_addr, port));
    log::info!(
        "Starting web server on {} ({})",
        address,
        if disable_auth {
            "localhost only - MCP mode"
        } else {
            "network accessible - production mode"
        }
    );
    let listener = TcpListener::bind(&address).await?;
    axum::serve(listener, app.into_make_service()).await
}
