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

mod module;
mod aws;
mod patch;

use module::Module;
use aws::{mutate_infra, read_status, create_queue_and_subscribe_to_topic};
use patch::patch_kind;

const FINALIZER_NAME: &str = "deletion-handler.finalizer.infrabridge.io";


#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    setup_logging().expect("Failed to initialize logging.");

    info!("This message will be logged to both stdout and the file.");
    
    let client: KubeClient = KubeClient::try_default().await?;
    let modules_api: Api<Module> = Api::namespaced(client.clone(), "default");
    let modules_watcher = watcher(modules_api, watcher::Config::default());

    // Shared state among watchers
    let watchers_state = Arc::new(Mutex::new(HashMap::new()));
    let specs_state: Arc<Mutex<HashMap<String, Value>>> = Arc::new(Mutex::new(HashMap::new()));

    let specs_state_clone = specs_state.clone(); // Clone the Arc before moving it into the async closure

    tokio::spawn(async move {
        create_queue_and_subscribe_to_topic("arn:aws:sns:eu-central-1:053475148537:events-topic-eu-central-1-dev".to_string(), specs_state_clone).await.unwrap();
    });

    modules_watcher.for_each(|event| async {
        match event {
            Ok(watcher::Event::Deleted(module)) => {
                info!("Deleted module: {}", module.spec.module_name);
                remove_module_watcher(client.clone(), module, watchers_state.clone()).await;
            },
            Ok(watcher::Event::Restarted(modules)) => {
                let module_names: String = modules.iter().map(|m| m.spec.module_name.clone()).collect::<Vec<_>>().join(",");
                info!("Restarted modules: {}", module_names);
                for module in modules {
                    add_module_watcher(client.clone(), module, watchers_state.clone(), specs_state.clone()).await;
                }
            },
            Ok(watcher::Event::Applied(module)) => {
                info!("Applied module: {}", module.spec.module_name);
                add_module_watcher(client.clone(), module, watchers_state.clone(), specs_state.clone()).await;
            },
            Err(e) => {
                // Handle error
                info!("Error: {:?}", e);
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

    info!("Watching for changes on kind: {}", &kind);

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
                    let namespace = crd.metadata.namespace.unwrap_or_else(|| "default".to_string());
                    
                    warn!("MUTATE_INFRA inside deletion");
                    let _ = mutate_infra(
                        event, 
                        kind.clone(), 
                        name.clone(), 
                        deployment_id, 
                        spec.clone(), 
                        annotations_value
                    ).await;
                    
                    let mut deletion_json = serde_json::json!({
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
                // Convert `BTreeMap<String, String>` to `serde_json::Value` using `.into()`
                let annotations_value = serde_json::json!(annotations);
                let plural = kind.to_lowercase() + "s"; // pluralize, this is a aligned in the crd-generator
                let namespace = crd.metadata.namespace.unwrap_or_else(|| "default".to_string());

                warn!("Annotations: {:?}", annotations);
                let is_deleting_annotation_present = annotations.get("deleting").map(|s| s == "true").unwrap_or(false);

                // let is_deleting = specs_state.lock().await.get(&name).map(|v| v == "0").unwrap_or(false);

                info!("ResourceStatus: {}", resource_status);
                match resource_status { // TODO: Use typed enum instead of string
                    "" => {
                        warn!("Will mutate infra for: deployment_id: {}, kind: {}, name: {}", deployment_id, kind, name);
                        let _ = mutate_infra(
                            event, 
                            kind.clone(), 
                            name.clone(), 
                            deployment_id, 
                            spec.clone(), 
                            annotations_value
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
                info!("Event: {:?}", event);
                info!("Error: {:?}", e);
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