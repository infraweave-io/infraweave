use env_common::list_modules;
use env_common::logic::{get_change_record, is_deployment_in_progress, read_logs, run_claim};
use futures::TryStreamExt;
use k8s_openapi::api::core::v1::Secret;
use k8s_openapi::apiextensions_apiserver::pkg::apis::apiextensions::v1::CustomResourceDefinition;
use k8s_openapi::ByteString;
use kube::api::{ApiResource, DynamicObject, PostParams};
use kube::{api::Api, runtime::watcher, Client as KubeClient};
use std::collections::{BTreeMap, HashSet};

use log::{info, warn};
use futures::stream::StreamExt;

use crate::apply::apply_module_crd;
use crate::defs::{FINALIZER_NAME, KUBERNETES_GROUP};

use kube::api::{Patch, PatchParams, ResourceExt};
use serde_json::{json, Error};

pub async fn start_operator() -> Result<(), Box<dyn std::error::Error>> {
    let client = initialize_kube_client().await?;

    let current_enviroment =
        std::env::var("INFRAWEAVE_ENVIRONMENT").unwrap_or_else(|_| "dev".to_string());
    info!("Current environment: {}", current_enviroment);

    let modules_watched_set: HashSet<String> = HashSet::new();
    list_and_apply_modules(client.clone(), &current_enviroment, &modules_watched_set).await;
    tokio::signal::ctrl_c().await?;

    Ok(())
}

async fn watch_all_infraweave_resources(
    client: kube::Client,
    kind: String,
) -> Result<(), Box<dyn std::error::Error>> {
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
                let namespace = resource.namespace().unwrap_or_else(|| "default".to_string());
                let environment = format!("{}/{}", cluster_name, namespace);
                
                println!("Resource applied: {:?}", resource);
                if resource.metadata.deletion_timestamp.is_none() {
                    println!("Resource is not being deleted");
                    // Resource is not being deleted
                    if !resource.finalizers().contains(&FINALIZER_NAME.to_string()) {
                        // Add the finalizer
                        let patch_params = PatchParams::default();
                        let patch = json!({
                            "metadata": {
                                "finalizers": [FINALIZER_NAME]
                            }
                        });
                        let namespaced_api = Api::<DynamicObject>::namespaced_with(
                            client.clone(),
                            &namespace,
                            &api_resource,
                        );
                        namespaced_api.patch(
                            &resource.metadata.name.clone().unwrap(),
                            &patch_params,
                            &Patch::Merge(&patch),
                        )
                        .await?;
                        println!("Added finalizer to {:?}", resource.metadata.name.unwrap());
                    } else {

                        println!("Resource has finalizer");
                        println!("Checking if resource has different lastGeneration");

                        if let Some(status) = resource.data.get("status") {
                            let observed_generation = status.get("lastGeneration").and_then(|g| g.as_i64()).unwrap_or(0);
                            let metadata_generation = resource.metadata.generation.unwrap_or(0);
                        
                            if observed_generation == metadata_generation {
                                println!("Generation has not changed; skipping reconciliation for {}", resource.metadata.name.unwrap());
                                continue;
                            } else {
                                println!("Generation has changed from {} to {}; reconciling", observed_generation, metadata_generation);
                            }
                        }

                        // Process the resource normally
                        let yaml = serde_yaml::to_value(&resource).unwrap();
                        println!("Applying {} manifest \n{:?}", kind, resource);
                        let (job_id, deployment_id) = match run_claim(&yaml, &environment, "apply").await {
                            Ok((job_id, deployment_id)) => {
                                println!("Successfully applied {} manifest", kind);
                                Ok((job_id, deployment_id))
                            }
                            Err(e) => {
                                Err(anyhow::anyhow!("Failed to apply {} manifest: {:?}", kind, e))
                            }
                        }.unwrap();
                        
                        follow_job_until_finished(
                            client.clone(),
                            &resource,
                            &api_resource,
                            job_id.as_str(),
                            deployment_id.as_str(),
                            &environment,
                            "Apply",
                        ).await.unwrap();
                    }
                } else {
                    // Resource is being deleted
                    if resource.finalizers().contains(&FINALIZER_NAME.to_string()) {
                        // Perform cleanup before deletion
                        let yaml = serde_yaml::to_value(&resource).unwrap();
                        println!("Deleting {} manifest \n{:?}", kind, resource);
                        let (job_id, deployment_id) = match run_claim(&yaml, &environment, "destroy").await {
                            Ok((job_id, deployment_id)) => {
                                println!("Successfully requested destroying {} manifest", kind);
                                update_resource_status(
                                    client.clone(),
                                    &resource,
                                    &api_resource,
                                    "Deleted",
                                    "Resource deleted successfully",
                                )
                                .await?;
                                Ok((job_id, deployment_id))
                            }
                            Err(e) => {
                                Err(anyhow::anyhow!("Failed to request destroying {} manifest: {:?}", kind, e))
                            }
                        }.unwrap();
                        
                        follow_job_until_finished(
                            client.clone(),
                            &resource,
                            &api_resource,
                            job_id.as_str(),
                            deployment_id.as_str(),
                            &environment,
                            "Delete",
                        ).await.unwrap();

                        // Remove the finalizer to allow deletion
                        let finalizers: Vec<String> = resource
                            .finalizers()
                            .into_iter()
                            .filter(|f| *f != FINALIZER_NAME)
                            .cloned()
                            .collect();
                        let patch_params = PatchParams::default();
                        let patch = json!({
                            "metadata": {
                                "finalizers": finalizers
                            }
                        });
                        let namespaced_api = Api::<DynamicObject>::namespaced_with(
                            client.clone(),
                            &namespace,
                            &api_resource,
                        );
                        namespaced_api.patch(
                            &resource.metadata.name.clone().unwrap(),
                            &patch_params,
                            &Patch::Merge(&patch),
                        )
                        .await?;
                        println!("Removed finalizer from {}", &resource.metadata.name.unwrap());
                    }
                }
            }
            watcher::Event::Deleted(_) => {
                // Resource has been fully deleted
                // You can perform any necessary cleanup here if needed
            }
            watcher::Event::Restarted(resources) => {
                for resource in resources {
                    println!("Acknowledging existence of {} resource: {:?}", kind, resource);
                    // Optionally call reconcile logic here
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
) -> Result<(), Box<dyn std::error::Error + Send + Sync + 'static>> {
    let available_modules = list_modules(&environment.to_string())
        .await
        .unwrap();

    let client2 = client.clone();
    for module in available_modules {
        if modules_watched_set.contains(&module.module) {
            warn!("Module {} already being watched", module.module);
            continue;
        }
        
        let client = client.clone();
        tokio::spawn(async move {

            match apply_module_crd(client.clone(), &module.manifest).await {
                Ok(_) => {
                    println!("Applied CRD for module {}", module.module);
                }
                Err(e) => {
                    eprintln!("Failed to apply CRD for module {}: {:?}", module.module, e);
                }
            }
    
            wait_for_crd_to_be_ready(client.clone(), &module.module).await;

            match watch_all_infraweave_resources(client.clone(), module.module.clone()).await {
                Ok(_) => {
                    println!("Watching resources for module {}", module.module);
                }
                Err(e) => {
                    println!("Failed to watch resources for module {}: {:?}", module.module, e);
                }
            }
        });
    }
    Ok(())
}

async fn wait_for_crd_to_be_ready(
    client: kube::Client,
    module: &str,
){
    // Wait until the CRD is established
    let crd_name = format!("{}s.infraweave.io", module);
    let crds: Api<CustomResourceDefinition> = Api::all(client.clone());

    // Retry loop to check if CRD is established
    for attempt in 0..10 {
        match crds.get(&crd_name).await {
            Ok(crd) => {
                if let Some(status) = crd.status {
                    if status.conditions.unwrap_or(vec![]).iter().any(|cond| cond.type_ == "Established" && cond.status == "True") {
                        println!("CRD {} is established.", crd_name);
                        break;
                    }
                }
            }
            Err(e) => {
                eprintln!("Error getting CRD {}: {:?}", crd_name, e);
            }
        }
        println!("CRD {} not yet established. Retrying...", crd_name);
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    }
}


async fn update_resource_status(
    client: kube::Client,
    resource: &DynamicObject,
    api_resource: &ApiResource,
    status: &str,
    message: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let namespace = resource.namespace().unwrap_or_else(|| "default".to_string());
    let namespaced_api = Api::<DynamicObject>::namespaced_with(client, &namespace, api_resource);

    println!(
        "ApiResource details: group='{}', version='{}', kind='{}', plural='{}'",
        api_resource.group, api_resource.version, api_resource.kind, api_resource.plural
    );
    println!(
        "Updating status for resource '{}' in namespace '{}'",
        &resource.metadata.name.clone().unwrap(),
        namespace
    );    
    
    let now = chrono::Utc::now().to_rfc3339();

    let status_patch = json!({
        "status": {
            "resourceStatus": status,
            "lastStatusUpdate": now,
            "lastGeneration": resource.metadata.generation.unwrap_or_default(),
            "logs": message,
        }
    });

    let patch_params = PatchParams::default();

    namespaced_api
        .patch_status(
            &resource.metadata.name.clone().unwrap(),
            &patch_params,
            &Patch::Merge(&status_patch),
        )
        .await?;

    println!("Updated status for {}", &resource.metadata.name.clone().unwrap());
    Ok(())
}

async fn create_secret(client: &kube::Client, namespace: &str) -> Result<(), Box<dyn std::error::Error>> {

    let secret_data = BTreeMap::from([
        ("username".to_string(), ByteString(base64::encode("my-username").into_bytes())),
        ("password".to_string(), ByteString(base64::encode("my-password").into_bytes())),
    ]);

    let secret_name = format!("infraweave-secret-test1");

    let secret = Secret {
        metadata: kube::api::ObjectMeta {
            name: Some(secret_name),
            namespace: Some(namespace.to_string()),
            ..Default::default()
        },
        data: Some(secret_data),
        ..Default::default()
    };

    let secrets: Api<Secret> = Api::namespaced(client.clone(), namespace);
    let pp = PostParams::default();
    let result = secrets.create(&pp, &secret).await?;

    println!("Stored secret {:?} in namespace {}", result.metadata.name, namespace);
    Ok(())
}

async fn follow_job_until_finished(
    client: kube::Client,
    resource: &DynamicObject,
    api_resource: &ApiResource,
    job_id: &str,
    deployment_id: &str,
    environment: &str,
    event: &str,
) -> Result<(), anyhow::Error> {
    let change_type = "APPLY";

    // Polling loop to check job statuses periodically until all are finished
    let mut deployment_status = "".to_string();
    loop {
        let (in_progress, job_id, depl_status) = is_deployment_in_progress(deployment_id, environment).await;
        deployment_status = depl_status;
        let status = if in_progress { "in progress" } else { "completed" };
        let event_status = format!("{} - {}", event, status);
        if in_progress {
            println!("Status of job {}: {}", job_id, status);
        } else {
            println!("Job is now finished!");
            break;
        }

        let log_str = match read_logs(&job_id).await {
            Ok(logs) => {
                let mut log_str = String::new();
                // take the last 10 logs
                for log in logs.iter().rev().take(10).rev() {
                    log_str.push_str(&format!("{}\n", log.message));
                }
                log_str
            },
            Err(e) => e.to_string(),
        };

        match update_resource_status(
            client.clone(),
            resource,
            &api_resource,
            &event_status,
            &log_str,
        ).await{
            Ok(_) => {
                println!("Updated status for resource {}", resource.metadata.name.clone().unwrap());
            }
            Err(e) => {
                println!("Failed to update status for resource: {:?}", e);
            }
        };

        std::thread::sleep(std::time::Duration::from_secs(10));
    }

    println!("Fetching change record for deployment {} in environment {}", deployment_id, environment);

    let change_record = match get_change_record(environment, deployment_id, job_id, change_type).await {
        Ok(change_record) => {
            println!("Change record for deployment {} in environment {}:\n{}", deployment_id, environment, change_record.plan_std_output);
            Ok(change_record)
        }
        Err(e) => {
            println!("Failed to get change record: {:?}", e);
            Err(anyhow::anyhow!("Failed to get change record: {:?}", e))
        }
    };

    match update_resource_status(
        client.clone(),
        resource,
        &api_resource,
        &format!("{} - {}", event, deployment_status),
        change_record.unwrap().plan_std_output.as_str(),
    ).await{
        Ok(_) => {
            println!("Updated status for resource {}", resource.metadata.name.clone().unwrap());
        }
        Err(e) => {
            println!("Failed to update status for resource: {:?}", e);
        }
    };

    Ok(())
}