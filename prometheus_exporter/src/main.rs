use std::collections::HashSet;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use axum::routing::get;
use axum::Router;
use endpoint::metrics_handler;
use env_common::interface::{initialize_project_id_and_region, CloudHandler};
use env_common::logic::handler;
use env_utils::setup_logging;

mod metrics;
mod endpoint;
use metrics::Metrics;


#[tokio::main]
async fn main() {
    setup_logging().unwrap();
    initialize_project_id_and_region().await;
    let metrics = Metrics::new();

    let (available_modules, available_stacks) = get_available_modules_stacks().await;

    let mut available_module_stacks = available_modules;
    available_module_stacks.extend(available_stacks);

    let available_modules = Arc::new(Mutex::new(available_module_stacks));

    let app = Router::new()
        .route("/metrics", get(move || metrics_handler(metrics.clone(), available_modules.clone())));

    let addr = SocketAddr::from(([0, 0, 0, 0], 3001));
    println!("Listening on {}", addr);
    axum::Server::bind(&addr)
        .serve(app.into_make_service())
        .await
        .unwrap();
}


async fn get_available_modules_stacks() -> (HashSet<String>, HashSet<String>) {
    initialize_project_id_and_region().await;
    let handler = handler();
    let (modules, stacks) = tokio::join!(
        handler.get_all_latest_module(""),
        handler.get_all_latest_stack("")
    );

    let unique_module_names: HashSet<_> = modules.unwrap_or(vec![])
        .into_iter()
        .map(|module| module.module)
        .collect();
    let unique_stack_names: HashSet<_> = stacks.unwrap_or(vec![])
        .into_iter()
        .map(|stack| stack.module)
        .collect();
    
    (unique_module_names, unique_stack_names)
}