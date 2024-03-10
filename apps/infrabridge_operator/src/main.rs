use aws_config::meta::region::RegionProviderChain;
use chrono::DateTime;
use chrono::Utc;
use env_aws::list_latest;
use kube::{
    api::Api, runtime::watcher, Client as KubeClient
  };
use serde_json::Value;
use std::{collections::HashMap, sync::Arc};
use tokio::sync::Mutex;
use kube_runtime::watcher::Event;

use log::{debug, info, warn, error, LevelFilter};
use chrono::Local;

use futures::stream::StreamExt;
use kube::api::DynamicObject;
use kube::api::GroupVersionKind;
use kube::api::ApiResource;

use tokio::time::{self, Duration};

use std::collections::BTreeMap;

use env_aws::{mutate_infra, read_status, create_queue_and_subscribe_to_topic};
mod patch;
use patch::patch_kind;

mod module;
use module::Module;

mod apply;
use crate::apply::{apply_module_crd, apply_module_kind};

const FINALIZER_NAME: &str = "deletion-handler.finalizer.infrabridge.io";

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    setup_logging().expect("Failed to initialize logging.");

    info!("This message will be logged to both stdout and the file.");
    let client: KubeClient = KubeClient::try_default().await?;

    let current_enviroment = std::env::var("INFRABRIDGE_ENVIRONMENT").unwrap_or_else(|_| "dev".to_string());
    info!("Current environment: {}", current_enviroment);

    let available_modules = match list_latest(&current_enviroment).await {
        Ok(m) => m,
        Err(e) => {
            error!("Failed to list latest modules: {}", e);
            return Err(e.into());
        }
    };
    warn!("Available modules: {:?}", available_modules);

    for module in available_modules {
        // Apply the CRD
        apply_module_kind(client.clone(), &module.manifest).await.expect("Failed to apply Module kind");
        apply_module_crd(client.clone(), &module.manifest).await.expect("Failed to apply CRD");
    }
    
    let modules_api: Api<Module> = Api::all(client.clone());
    let modules_watcher = watcher(modules_api, watcher::Config::default());

    // Shared state among watchers
    let watchers_state = Arc::new(Mutex::new(HashMap::new()));
    let specs_state: Arc<Mutex<HashMap<String, Value>>> = Arc::new(Mutex::new(HashMap::new()));
    
    let specs_state_clone = specs_state.clone(); // Clone the Arc before moving it into the async closure
    
    tokio::spawn(async move {
        let queue_url = create_queue_and_subscribe_to_topic("arn:aws:sns:eu-central-1:053475148537:events-topic-eu-central-1-dev".to_string()).await.unwrap();
        let _ = poll_sqs_messages(queue_url.to_string(), specs_state_clone).await;
    });
    
    modules_watcher.for_each(|event| async {
        match event {
            Ok(watcher::Event::Deleted(module)) => {
                warn!("Deleted module: {}", module.spec.module_name);
                remove_module_watcher(client.clone(), module, watchers_state.clone()).await;
            },
            Ok(watcher::Event::Restarted(modules)) => {
                let module_names: String = modules.iter().map(|m| m.spec.module_name.clone()).collect::<Vec<_>>().join(",");
                warn!("Restarted modules: {}", module_names);
                for module in modules {
                    add_module_watcher(client.clone(), module, watchers_state.clone(), specs_state.clone()).await;
                }
            },
            Ok(watcher::Event::Applied(module)) => {
                warn!("Applied module: {}", module.spec.module_name);
                add_module_watcher(client.clone(), module, watchers_state.clone(), specs_state.clone()).await;
            },
            Err(e) => {
                // Handle error
                warn!("Error: {:?}", e);
            },
        }
    }).await;

    Ok(())
}

async fn remove_module_watcher(
    _client: KubeClient,
    module: Module,
    watchers_state: Arc<Mutex<HashMap<String, ()>>>,
) {
    let kind = module.spec.module_name.clone();
    let mut watchers = watchers_state.lock().await;
    watchers.remove(&kind);
}

async fn add_module_watcher(
    client: KubeClient,
    module: Module,
    _watchers_state: Arc<Mutex<HashMap<String, ()>>>,
    specs_state: Arc<Mutex<HashMap<String, Value>>>,
) {
    let kind = module.spec.module_name.clone();
    let mut watchers = _watchers_state.lock().await;
    
    if !watchers.contains_key(&kind) {
        info!("Adding watcher for kind: {}", &kind);
        let client_clone = client.clone();
        let kind_clone = kind.clone();
        let watchers_state_clone = _watchers_state.clone();
        tokio::spawn(async move {
            watch_for_kind_changes(client_clone, kind_clone, watchers_state_clone, specs_state).await;
        });

        watchers.insert(kind, ());
    }else{
        info!("Watcher already exists for kind: {}", &kind);
    }
}


async fn watch_for_kind_changes(
    client: KubeClient,
    kind: String,
    _watchers_state: Arc<Mutex<HashMap<String, ()>>>,
    specs_state: Arc<Mutex<HashMap<String, Value>>>,
) {
    let gvk = GroupVersionKind::gvk("infrabridge.io", "v1", &kind);
    let resource = ApiResource::from_gvk(&gvk);
    let api: Api<DynamicObject> = Api::all_with(client.clone(), &resource);

    warn!("Watching for changes on kind: {}", &kind);

    let kind_watcher = watcher(api.clone(), watcher::Config::default());
    kind_watcher.for_each(|event| async {
        match event {
            Ok(Event::Applied(crd)) => {
                warn!("Event::Applied crd: {:?}", crd);
                let annotations = crd.metadata.annotations.clone().unwrap_or_else(|| BTreeMap::new());
                let status = crd.data.get("status").and_then(|s| serde_json::from_value::<BTreeMap<String, serde_json::Value>>(s.clone()).ok()).unwrap_or_else(|| BTreeMap::new());
                
                let spec = crd.data.get("spec").unwrap();
                let deployment_id = annotations.get("deploymentId").map(|s| s.clone()) // Clone the string if found
                .unwrap_or("".to_string()); // Provide an owned empty String as the default
                let prev_spec = specs_state.lock().await.get(&deployment_id).map(|v| v.clone()).unwrap_or_else(|| serde_json::json!({}));

                let no_spec_change = &prev_spec == spec && deployment_id != "";

                if no_spec_change && !crd.metadata.deletion_timestamp.is_some(){
                    warn!("No change in specs for: kind: {}, name: {}", &kind, crd.metadata.name.unwrap_or_else(|| "noname".to_string()));
                    return;
                } else if crd.metadata.deletion_timestamp.is_some() {
                    info!("Item is marked for deletion, checking if already sent destroy query");
                    
                    let deletion_key = get_deletion_key(deployment_id.clone());
                    let deletion_json = specs_state.lock().await.get(&deletion_key).map(|v| v.clone()).unwrap_or_else(|| serde_json::json!({}));

                    if deletion_json.get("deleting").map(|v| v == "true").unwrap_or(false) {
                        warn!("Item is marked for deletion and already sent destroy query");
                        return;
                    }
                    
                    let event = "destroy".to_string();
                    let deployment_id = crd.metadata.annotations.as_ref()
                        .and_then(|annotations| annotations.get("deploymentId").map(|s| s.clone())) // Clone the string if found
                        .unwrap_or("".to_string()); // Provide an owned empty String as the default
                    let spec = crd.data.get("spec").unwrap();
                    let annotations = crd.metadata.annotations.unwrap_or_else(|| BTreeMap::new());
                    // Convert `BTreeMap<String, String>` to `serde_json::Value` using `.into()`
                    let annotations_value = serde_json::json!(annotations);
                    let name = crd.metadata.name.unwrap_or_else(|| "noname".to_string());
                    // Convert `BTreeMap<String, String>` to `serde_json::Value` using `.into()`
                    let annotations_value = serde_json::json!(annotations);
                    let plural = kind.to_lowercase() + "s"; // pluralize, this is a aligned in the crd-generator
                    // let namespace = crd.metadata.namespace.unwrap_or_else(|| "default".to_string());
                    
                    warn!("MUTATE_INFRA inside deletion");
                    let deployment_id = match mutate_infra(
                        event, 
                        kind.clone(), 
                        name.clone(), 
                        deployment_id, 
                        spec.clone(), 
                        annotations_value
                    ).await {
                        Ok(id) => id,
                        Err(e) => {
                            error!("Failed to mutate infra: {}", e);
                            return;
                        }
                    };

                    let module = kind.clone();
                    // Get the current time in UTC
                    let now: DateTime<Utc> = Utc::now();
                    // Format the timestamp to RFC 3339 without microseconds
                    let timestamp = now.format("%Y-%m-%dT%H:%M:%S%:z").to_string();

                    patch_kind(
                        KubeClient::try_default().await.unwrap(),
                        deployment_id.to_string(),
                        module.clone(),
                        name.clone(),
                        module.to_lowercase() + "s",
                        "default".to_string(),
                        serde_json::json!({
                            "metadata": {
                                "annotations": {
                                    "deploymentId": deployment_id,
                                }
                            },
                            "status": {
                                "resourceStatus": "queried",
                                "lastStatusUpdate": timestamp,
                            }
                        })
                    ).await;
                    
                    let deletion_json = serde_json::json!({
                        "deleting": "true"
                    });
                    specs_state.lock().await.insert(deletion_key.clone(), deletion_json.clone());
                    

                    return;
                } else {
                    warn!("Current spec: {:?}", spec);
                    warn!("Previous spec: {:?}", prev_spec);
                }
                specs_state.lock().await.insert(deployment_id.clone(), spec.clone());

                // Check resourceStatus as this determines current state of the resource
                // and what action to take
                let resource_status = status.get("resourceStatus")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");

                // Get some data from the CRD
                let name = crd.metadata.name.unwrap_or_else(|| "noname".to_string());
                info!("Applied {}: {}, data: {:?}", &kind, name, crd.data);
                let event = "apply".to_string();
                let annotations_value = serde_json::json!(annotations);
                // let plural = kind.to_lowercase() + "s"; // pluralize, this is a aligned in the crd-generator
                // let namespace = crd.metadata.namespace.unwrap_or_else(|| "default".to_string());

                warn!("Annotations: {:?}", annotations);
                // let is_deleting_annotation_present = annotations.get("deleting").map(|s| s == "true").unwrap_or(false);

                info!("ResourceStatus: {}", resource_status);
                match resource_status { // TODO: Use typed enum instead of string
                    "" => {
                        warn!("Will mutate infra for: deployment_id: {}, kind: {}, name: {}", deployment_id, kind, name);
                        let new_deployment_id = match mutate_infra(
                            event, 
                            kind.clone(), 
                            name.clone(), 
                            deployment_id, 
                            spec.clone(), 
                            annotations_value
                        ).await {
                            Ok(id) => id,
                            Err(e) => {
                                error!("Failed to mutate infra: {}", e);
                                return;
                            }
                        };

                        let module = kind.clone();
                        // Get the current time in UTC
                        let now: DateTime<Utc> = Utc::now();
                        // Format the timestamp to RFC 3339 without microseconds
                        let timestamp = now.format("%Y-%m-%dT%H:%M:%S%:z").to_string();

                        patch_kind(
                            KubeClient::try_default().await.unwrap(),
                            new_deployment_id.to_string(),
                            module.clone(),
                            name.clone(),
                            module.to_lowercase() + "s",
                            "default".to_string(),
                            serde_json::json!({
                                "metadata": {
                                    "annotations": {
                                        "deploymentId": new_deployment_id,
                                    }
                                },
                                "status": {
                                    "resourceStatus": "queried",
                                    "lastStatusUpdate": timestamp,
                                }
                            })
                        ).await;
                      
                        // schedule_status_check(
                        //     5, 
                        //     "S3Bucket-my-s3-bucket-c7q".to_string(),
                        // );
                    },
                    "Creating" => {
                        // Set up periodic checks for status

                        // let infra_status = get_infrabridge_status(deployment_id.clone()).await;
                        // schedule_status_check(
                        //     5, 
                        //     "S3Bucket-my-s3-bucket-c7q".to_string(),
                        // );
                    },
                    "Deployed" => {

                        // Setting status to deployed again to update the lastStatusUpdate time
                        // set_status_for_cr(
                        //     client.clone(), 
                        //     kind.clone(), 
                        //     name.clone(), 
                        //     plural,
                        //     namespace,
                        //     "Deployed".to_string()
                        // ).await;

                        // schedule_status_check(
                        //     15, 
                        //     "S3Bucket-my-s3-bucket-c7q".to_string(),
                        // );
                    },
                    _ => {
                        info!("ResourceStatus: {}", resource_status);
                    }
                }
            },
            Ok(Event::Restarted(crds)) => {
                warn!("Event::Restarted crds: {:?}", crds);
                for crd in crds {
                    let name = crd.metadata.name.unwrap_or_else(|| "noname".to_string());
                    info!("Restarted {}: {}, data: {:?}", &kind, name, crd.data);

                    let plural = kind.to_lowercase() + "s"; // pluralize, this is a aligned in the crd-generator
                    let namespace = crd.metadata.namespace.unwrap_or_else(|| "default".to_string());
                    let deployment_id = crd.metadata.annotations.as_ref()
                        .and_then(|annotations| annotations.get("deploymentId").map(|s| s.clone())) // Clone the string if found
                        .unwrap_or("".to_string()); // Provide an owned empty String as the default

                    if !crd.metadata.deletion_timestamp.is_some() && !crd.metadata.finalizers.as_ref().map(|f| f.contains(&FINALIZER_NAME.to_string())).unwrap_or(false) {
                        warn!("item is not marked for deletion, ensuring finalizer is set");
                        
                        patch_kind(
                            client.clone(),
                            deployment_id.clone(),
                            kind.clone(), 
                            name.clone(), 
                            plural,
                            namespace,
                            serde_json::json!({
                                "metadata": {
                                    "finalizers": [FINALIZER_NAME.to_string()]
                                }
                            })
                        ).await;
                    }
                }
            },
            Ok(Event::Deleted(crd)) => {
                let name = crd.metadata.name.unwrap_or_else(|| "noname".to_string());
                warn!("Event::Deleted {}: {}, data: {:?}", &kind, name, crd.data);



                let finalizers = crd.metadata.finalizers.clone().unwrap_or_else(|| Vec::new());
                info!("Finalizers: {:?}", finalizers);
                if crd.metadata.deletion_timestamp.is_some() && finalizers.contains(&FINALIZER_NAME.to_string()){
                    info!("item is marked for deletion and has finalizer");
                }
            },
            Err(ref e) => {
                warn!("Event: {:?}", event);
                warn!("Error: {:?}", e);
            },
        }
    }).await;
}


async fn periodic_status_check(delay_seconds: u64, deployment_id: String) {
    let mut interval = time::interval(Duration::from_secs(delay_seconds));
    
    loop {
        interval.tick().await;
        // Execute the task
        match read_status(deployment_id.clone()).await {
            Ok(status) => info!("Status: {:?}", status),
            Err(e) => error!("Failed to read status: {:?}", e),
        }
    }
}

fn schedule_status_check(delay_seconds: u64, deployment_id: String) {
    // Schedule a status check
    info!("Scheduling future job...");

    // Spawn a new asynchronous task for the delayed job
    tokio::spawn(async move {
        tokio::time::sleep(tokio::time::Duration::from_secs(delay_seconds)).await;

        // After the delay, run the future job
        let status = match read_status(deployment_id).await {
            Ok(status) => status,
            Err(e) => {
                error!("Failed to read status: {:?}", e);
                return;
            },
        };
        info!("Status: {:?}", status);
    });
}

fn setup_logging() -> Result<(), fern::InitError> {
    let base_config = fern::Dispatch::new();

    let stdout_config = fern::Dispatch::new()
        .format(|out, message, record| {
            out.finish(format_args!(
                "{}[{}] {}: {}",
                Local::now().format("[%Y-%m-%d][%H:%M:%S]"),
                record.target(),
                record.level(),
                message
            ))
        })
        .level(LevelFilter::Warn)
        .chain(std::io::stdout());

    // let file_config = fern::Dispatch::new()
    //     .format(|out, message, record| {
    //         out.finish(format_args!(
    //             "{}[{}] {}: {}",
    //             Local::now().format("[%Y-%m-%d][%H:%M:%S]"),
    //             record.target(),
    //             record.level(),
    //             message
    //         ))
    //     })
    //     .level(LevelFilter::Info)
    //     .chain(fern::log_file("output.log")?);

    base_config
        .chain(stdout_config)
        // .chain(file_config)
        .apply()?;

    Ok(())
}

pub fn get_deletion_key(deployment_id: String) -> String {
    format!("{}-{}", deployment_id, "deleting")
}


async fn delete_kind_finalizer(
    client: KubeClient,
    kind: String,
    name: String,
    plural: String,
    namespace: String,
    specs_state: Arc<Mutex<HashMap<String, Value>>>,
    deployment_id: String,
) {
    warn!("Deleting kind finalizer for: kind: {}, name: {}, plural: {}, namespace: {}", &kind, name, plural, namespace);
    let api_resource = ApiResource::from_gvk_with_plural(
        &GroupVersionKind {
            group: "infrabridge.io".into(),
            version: "v1".into(),
            kind: kind.clone(),
        }, 
        &plural
    );
    let api: Api<DynamicObject> = Api::namespaced_with(client, &namespace, &api_resource);

    // Remove the deployment_id from the specs_state
    specs_state.lock().await.remove(&deployment_id);
    let deletion_key = get_deletion_key(deployment_id.clone());
    specs_state.lock().await.remove(&deletion_key);

    let resource = api.get(&name).await;
    match resource {
        Ok(res) => {
            let finalizers = res.metadata.finalizers.unwrap_or_default();
            let finalizers_to_keep: Vec<String> = finalizers.into_iter().filter(|f| f != FINALIZER_NAME).collect();

            warn!("Finalizers after removing {}: {:?}", FINALIZER_NAME, finalizers_to_keep);

            let patch = serde_json::json!({
                "metadata": {
                    "finalizers": finalizers_to_keep,
                    "resourceVersion": res.metadata.resource_version,
                }
            });

            let params = kube::api::PatchParams::default();
            match api.patch(&name, &params, &kube::api::Patch::Merge(&patch)).await {
                Ok(_) => warn!("Finalizer removed for: kind: {}, name: {}, plural: {}, namespace: {}", &kind, name, plural, namespace),
                Err(e) => warn!("Error deleting finalizer: {}", e)
            }
        },
        Err(e) => warn!("Error fetching resource: {}", e),
    }

}


pub fn status_check(deployment_id: String, specs_state: Arc<Mutex<HashMap<String, Value>>>, kube_client: KubeClient) {
    // Schedule a status check
    info!("Fetching status for event with deployment_id {}...", deployment_id);

    // Spawn a new asynchronous task for the delayed job
    tokio::spawn(async move {
        let status_json = match read_status(deployment_id.clone()).await {
            Ok(status) => status,
            Err(e) => {
                error!("Failed to read status: {:?}", e);
                return;
            },
        };
        warn!("Status fetched for deployment_id: {:?}", status_json);
        // Read json from status
        info!("Would patch status for deployment_id: {} with {:?}", status_json.deployment_id, status_json);

        // Get the current time in UTC
        let now: DateTime<Utc> = Utc::now();
        // Format the timestamp to RFC 3339 without microseconds
        let timestamp = now.format("%Y-%m-%dT%H:%M:%S%:z").to_string();
        let info = format!("{}: {}", status_json.event, status_json.status);
        // If status_json.status is any of "received", "initiated", set in-progress to true
        let in_progress = if status_json.status == "received" || status_json.status == "initiated" {
            "true"
        } else {
            "false"
        };
        patch_kind(
            kube_client.clone(),
            deployment_id.clone(),
            status_json.module.clone(),
            status_json.name.clone(),
            status_json.module.clone().to_lowercase() + "s",
            "default".to_string(),
            serde_json::json!({
                "metadata": {
                    "annotations": {
                        "in-progress": in_progress,
                    }
                },
                "status": {
                    "resourceStatus": info,
                    "lastStatusUpdate": timestamp,
                }
            })
        ).await;

        if status_json.event == "destroy" && status_json.status == "finished" {
            delete_kind_finalizer(kube_client, status_json.module.clone(), status_json.name, status_json.module.to_lowercase() + "s", "default".to_string(), specs_state, deployment_id.clone()).await;
        } else{
            info!("Not deleting finalizer for: kind: {}, name: {}, plural: {}, namespace: {}", status_json.module, status_json.name, status_json.module.to_lowercase() + "s", "default");
        }
    });
}

use aws_sdk_sqs::Client as SqsClient;

async fn poll_sqs_messages(queue_url: String, specs_state: Arc<Mutex<HashMap<String, Value>>>) -> Result<(), Box<dyn std::error::Error>> {
    let region_provider = RegionProviderChain::default_provider().or_else("eu-central-1");
    let config = aws_config::from_env().region(region_provider).load().await;
    let sqs_client = SqsClient::new(&config);

    let kube_client = KubeClient::try_default().await?;

    info!("Polling for messages...");
    loop {
        let received_messages = sqs_client.receive_message()
            .queue_url(&queue_url)
            .wait_time_seconds(20) // Use long polling
            .send().await?;

        // Correctly handle the Option returned by received_messages.messages()
        for message in received_messages.messages.unwrap_or_default() {
            if let Some(body) = message.body() {
                if let Ok(outer_parsed) = serde_json::from_str::<Value>(body) {
                    // Access the "Message" field and parse it as JSON
                    if let Some(inner_message_str) = outer_parsed.get("Message").and_then(|m| m.as_str()) {
                        if let Ok(inner_parsed) = serde_json::from_str::<Value>(inner_message_str) {
                            // Now, extract the deployment_id from the inner JSON
                            if let Some(deployment_id) = inner_parsed.get("deployment_id").and_then(|d| d.as_str()) {
                                info!("Deployment ID: {:?}", deployment_id);

                                warn!("Received message: {:?}", inner_parsed);

                                status_check(deployment_id.to_string(), specs_state.clone(), kube_client.clone());
                            }
                        }
                    }
                }
            }

            debug!("Acking message: {:?}", message.body());

            if let Some(receipt_handle) = message.receipt_handle() {
                sqs_client.delete_message()
                    .queue_url(&queue_url)
                    .receipt_handle(receipt_handle)
                    .send().await?;
            }
        }

        tokio::time::sleep(Duration::from_secs(1)).await; // Sleep to prevent constant polling if no messages are available
    }
}
