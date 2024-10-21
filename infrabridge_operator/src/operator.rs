use env_common::list_modules;
use env_common::logic::run_claim;
use futures::TryStreamExt;
use kube::api::{ApiResource, DynamicObject};
use kube::{api::Api, runtime::watcher, Client as KubeClient};
use std::collections::HashSet;

use log::{info, warn};
use futures::stream::StreamExt;

use crate::apply::apply_module_crd;
use crate::defs::KUBERNETES_GROUP;

pub async fn start_operator() -> Result<(), Box<dyn std::error::Error>> {
    let client = initialize_kube_client().await?;

    let current_enviroment =
        std::env::var("INFRABRIDGE_ENVIRONMENT").unwrap_or_else(|_| "dev".to_string());
    info!("Current environment: {}", current_enviroment);

    let modules_watched_set: HashSet<String> = HashSet::new();
    list_and_apply_modules(client.clone(), &current_enviroment, &modules_watched_set).await?;
    tokio::signal::ctrl_c().await?;

    Ok(())
}

async fn watch_all_infrabridge_resources(client: &kube::Client, kind: &str) -> Result<(), Box<dyn std::error::Error>> {
    let api_resource = ApiResource {
        api_version: format!("{}/v1", KUBERNETES_GROUP),
        group: KUBERNETES_GROUP.to_string(),
        version: "v1".to_string(),
        kind: kind.to_string(),
        plural: (kind.to_lowercase() + "s").to_string(),
    };

    let api = Api::<DynamicObject>::all_with(client.clone(), &api_resource);
    let list_params = watcher::Config::default();
    let mut resource_watcher = watcher(api, list_params).boxed();

    let cluster_name = "my-k8s-cluster-1".to_string(); // TODO: Get cluster name from env

    while let Some(event) = resource_watcher.try_next().await? {
        match event {
            watcher::Event::Applied(resource) => {
                let default_namespace = "default".to_string();
                let namespace = resource.metadata.namespace.as_ref().unwrap_or(&default_namespace);
                let environment = format!("{}/{}", cluster_name, namespace);
                let yaml = serde_yaml::to_value(&resource).unwrap();
                println!("Applying {} manifest \n{:?}", kind, resource);
                match run_claim(&yaml, &environment, "apply").await {
                    Ok(_) => {
                        println!("Successfully applied {} manifest", kind);
                    }
                    Err(e) => {
                        println!("Failed to apply {} manifest: {:?}", kind, e);
                    }
                }
            }
            watcher::Event::Deleted(resource) => {
                let default_namespace = "default".to_string();
                let namespace = resource.metadata.namespace.as_ref().unwrap_or(&default_namespace);
                let environment = format!("{}/{}", cluster_name, namespace);
                let yaml = serde_yaml::to_value(&resource).unwrap();
                println!("Deleting {} manifest \n{:?}", kind, resource);
                match run_claim(&yaml, &environment, "destroy").await {
                    Ok(_) => {
                        println!("Successfully destroyed {} manifest", kind);
                    }
                    Err(e) => {
                        println!("Failed to destroy {} manifest: {:?}", kind, e);
                    }
                }
            }
            watcher::Event::Restarted(resources) => {
                for resource in resources {
                    println!("Acknowledging existence of {} resource: {:?}", kind, resource);
                }
            }
        }
    }
    Ok(())
}

async fn initialize_kube_client() -> Result<KubeClient, Box<dyn std::error::Error>> {
    Ok(KubeClient::try_default().await?)
}

async fn list_and_apply_modules(
    client: KubeClient,
    environment: &str,
    modules_watched_set: &HashSet<String>,
) -> Result<(), Box<dyn std::error::Error>> {
    let available_modules = list_modules(&environment.to_string())
        .await
        .unwrap();

    for module in available_modules {
        if modules_watched_set.contains(&module.module) {
            warn!("Module {} already being watched", module.module);
            continue;
        }
        // Generate a CRD and apply it to the cluster (e.g.: kind: S3Bucket, IAMRole, etc.)
        // All will have group: infrabridge.io
        println!("will apply module: {:?}", module.module);
        apply_module_crd(&client, &module.manifest).await?;

        watch_all_infrabridge_resources(&client, &module.module).await?;
    }
    Ok(())
}
