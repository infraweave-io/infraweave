use kube::{
    api::Api, Client,
    runtime::{watcher}
  };
use std::{collections::HashMap, sync::Arc};
use tokio::sync::Mutex;
use kube_runtime::watcher::Event;

use log::{info, error, LevelFilter};
use chrono::Local;

use futures::stream::StreamExt;
use kube::ResourceExt;
use kube::api::DynamicObject;
use kube::api::GroupVersionKind;
use kube::api::ApiResource;
use kube::api::Patch;
use kube::api::PatchParams;

use serde_json::json;

mod module;
mod aws;

use module::Module;
use aws::mutate_infra;


#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    setup_logging().expect("Failed to initialize logging.");

    info!("This message will be logged to both stdout and the file.");
    
    let client = Client::try_default().await?;
    let modules_api: Api<Module> = Api::namespaced(client.clone(), "default");
    let modules_watcher = watcher(modules_api, watcher::Config::default());

    // Shared state among watchers
    let watchers_state = Arc::new(Mutex::new(HashMap::new()));

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
                    add_module_watcher(client.clone(), module, watchers_state.clone()).await;
                }
            },
            Ok(watcher::Event::Applied(module)) => {
                info!("Applied module: {}", module.spec.module_name);
                add_module_watcher(client.clone(), module, watchers_state.clone()).await;
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
    _client: Client,
    module: Module,
    watchers_state: Arc<Mutex<HashMap<String, ()>>>,
) {
    let kind = module.spec.module_name.clone();
    let mut watchers = watchers_state.lock().await;
    watchers.remove(&kind);
}

async fn add_module_watcher(
    client: Client,
    module: Module,
    _watchers_state: Arc<Mutex<HashMap<String, ()>>>,
) {
    let kind = module.spec.module_name.clone();
    let mut watchers = _watchers_state.lock().await;
    
    if !watchers.contains_key(&kind) {
        let client_clone = client.clone();
        let kind_clone = kind.clone();
        let watchers_state_clone = _watchers_state.clone();
        tokio::spawn(async move {
            watch_for_kind_changes(client_clone, kind_clone, watchers_state_clone).await;
        });

        watchers.insert(kind, ());
    }else{
        info!("Watcher already exists for kind: {}", &kind);
    }
}


async fn watch_for_kind_changes(
    client: Client,
    kind: String,
    _watchers_state: Arc<Mutex<HashMap<String, ()>>>,
) {
    let gvk = GroupVersionKind::gvk("infrabridge.io", "v1", &kind);
    let resource = ApiResource::from_gvk(&gvk);
    let api: Api<DynamicObject> = Api::all_with(client.clone(), &resource);

    info!("Watching for changes on kind: {}", &kind);

    let kind_watcher = watcher(api, watcher::Config::default());
    kind_watcher.for_each(|event| async {
        match event {
            Ok(Event::Applied(crd)) => {
                let name = crd.metadata.name.unwrap_or_else(|| "noname".to_string());
                info!("Applied {}: {}, data: {:?}", &kind, name, crd.data);

                let event = "apply".to_string();
                let deployment_id = format!("s3bucket-marius-123");
                let spec = crd.data.get("spec").unwrap();
                let _ = mutate_infra(event, kind.clone(), name.clone(), deployment_id, spec.clone()).await;
                // wait 2 seconds
                tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
                set_status_for_cr(client.clone(), kind.clone(), name, "Deployed Boyeah!!".to_string()).await;
            },
            Ok(Event::Restarted(crds)) => {
                for crd in crds {
                    let name = crd.metadata.name.unwrap_or_else(|| "noname".to_string());
                    info!("Restarted {}: {}, data: {:?}", &kind, name, crd.data);
                }
            },
            Ok(Event::Deleted(crd)) => {
                let name = crd.metadata.name.unwrap_or_else(|| "noname".to_string());
                info!("Deleted {}: {}, data: {:?}", &kind, name, crd.data);

                let event = "destroy".to_string();
                let deployment_id = format!("s3bucket-marius-123");
                let spec = crd.data.get("spec").unwrap();
                let _ = mutate_infra(event, kind.clone(), name.clone(), deployment_id, spec.clone()).await;
            },
            Err(ref e) => {
                info!("Event: {:?}", event);
                info!("Error: {:?}", e);
            },
        }
    }).await;
}


async fn set_status_for_cr(
    client: Client,
    kind: String,
    name: String,
    status: String,
) {
    info!("Setting status for {}: {}", &kind, name);
    let gvk = GroupVersionKind::gvk("infrabridge.io", "v1", &kind);
    let resource = ApiResource::from_gvk(&gvk);
    let api: Api<DynamicObject> = Api::namespaced_with(client, "default", &resource);

    let cr = match api.get(&name).await {
        Ok(cr) => cr,
        Err(e) => {
            error!("Failed to get CR: {}", e);
            return;
        }
    };

    let name = cr.name_any();
    let new_status = json!({
        "status": {
            "resourceStatus": status,
        }
    });

    let patch = Patch::Merge(&new_status);
    let patch_params = PatchParams::default();


    info!("Patch being applied: {:?} for {}: {}", &new_status, &kind, &name);
    info!("Attempting to patch CR with GVK: {:?}, name: {}, namespace: {}", gvk, name, "default");

    match api.patch_status(&name, &patch_params, &patch).await {
        Ok(_) => info!("Successfully updated CR status for: {}", name),
        Err(e) => error!("Failed to update CR status for: {}: {:?}", name, e),
    }
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
        .level(LevelFilter::Info)
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