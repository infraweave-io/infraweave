use kube::{
    api::Api, Client,
    runtime::{watcher}
  };
use std::{collections::HashMap, sync::Arc};
use tokio::sync::Mutex;
use kube_runtime::watcher::Event;

use log::{info, LevelFilter};
use chrono::Local;

use futures::stream::StreamExt;

use kube::api::DynamicObject;
use kube::api::GroupVersionKind;
use kube::api::ApiResource;

mod module;
// mod crd;

use module::Module;
// use crd::GeneralCRD;


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
        // let event = event;.unwrap(); // Simplification, handle errors appropriately
        match event {
            Ok(watcher::Event::Deleted(module)) => {
                info!("Deleted module: {}", module.spec.moduleName);
                let kind = module.spec.moduleName.clone(); // Clone here for use inside tokio::spawn
                let mut watchers = watchers_state.lock().await;
                watchers.remove(&kind);
            },
            Ok(watcher::Event::Applied(module)) => {
                info!("Applied module: {}", module.spec.moduleName);
                let kind = module.spec.moduleName.clone(); // Clone here for use inside tokio::spawn

                let watchers_state = watchers_state.clone();
                let mut watchers = watchers_state.lock().await;

                if !watchers.contains_key(&kind) {
                    let client = client.clone();
                    let kind_clone = kind.clone(); // Clone again to move into tokio::spawn
                    tokio::spawn(async move {

                        let gvk = GroupVersionKind::gvk("infrabridge.io", "v1", &kind_clone);
                        let resource = ApiResource::from_gvk(&gvk);
                        let api: Api<DynamicObject> = Api::all_with(client.clone(), &resource);

                        let kind_watcher = watcher(api, watcher::Config::default());
                        kind_watcher.for_each(|event| async {
                            // Process kind-specific events here
                            match event {
                                Ok(Event::Applied(crd)) => {
                                    info!("Applied {}: {}, data: {}", &kind_clone, crd.metadata.name.unwrap_or_else(|| "noname".to_string()), crd.data);
                                },
                                Ok(Event::Deleted(crd)) => {
                                    info!("Deleted {}: {}, data: {}", &kind_clone, crd.metadata.name.unwrap_or_else(|| "noname".to_string()), crd.data);
                                },
                                Err(ref e) => {
                                    info!("Event: {:?}", event);
                                    info!("Error: {:?}", e);
                                },
                                _ => {
                                    info!("Unhandled: {:?}", event);
                                }
                            }
                        }).await;
                    });

                    watchers.insert(kind, ());
                }
            },
            Err(e) => {
                // Handle error
                info!("Error: {:?}", e);
            },
            _ => {
                // Handle other events
                info!("Unhandled: {:?}", event);
                
            }
        }
    }).await;

    Ok(())
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