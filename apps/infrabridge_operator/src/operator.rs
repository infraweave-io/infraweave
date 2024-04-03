use env_aws::list_module;
use kube::{
    api::Api, runtime::watcher, Client as KubeClient
  };
use serde_json::Value;
use std::{collections::HashMap, sync::Arc};
use tokio::sync::Mutex;

use log::{info, warn, error};

use futures::stream::StreamExt;

use env_aws::create_queue_and_subscribe_to_topic;

use crate::module::Module;
use crate::apply::{apply_module_crd, apply_module_kind};
use crate::other::{add_module_watcher, remove_module_watcher};
use crate::status::poll_sqs_messages;

pub async fn start_operator() -> Result<(), Box<dyn std::error::Error>>{

    let client: KubeClient = KubeClient::try_default().await?;

    let current_enviroment = std::env::var("INFRABRIDGE_ENVIRONMENT").unwrap_or_else(|_| "dev".to_string());
    info!("Current environment: {}", current_enviroment);

    let available_modules = match list_module(&current_enviroment).await {
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