use aws_sdk_sqs::types::Message;
use env_aws::list_module;
use kube::{api::Api, runtime::watcher, Client as KubeClient};
use serde_json::Value;
use std::{collections::HashMap, sync::Arc};
use tokio::sync::Mutex;

use log::{error, info, warn};

use futures::stream::StreamExt;

use env_aws::create_queue_and_subscribe_to_topic;

use crate::apply::{apply_module_crd, apply_module_kind};
use crate::crd::Module;
use crate::module::{add_module_watcher, remove_module_watcher};
use crate::status::{poll_sqs_messages, status_check, SqsMessageHandler};

pub async fn start_operator() -> Result<(), Box<dyn std::error::Error>> {
    let client = initialize_kube_client().await?;

    let current_enviroment =
        std::env::var("INFRABRIDGE_ENVIRONMENT").unwrap_or_else(|_| "dev".to_string());
    info!("Current environment: {}", current_enviroment);

    list_and_apply_modules(client.clone(), &current_enviroment).await?;

    // Shared state among watchers
    let watchers_state = Arc::new(Mutex::new(HashMap::new()));
    let specs_state: Arc<Mutex<HashMap<String, Value>>> = Arc::new(Mutex::new(HashMap::new()));

    spawn_message_polling(specs_state.clone()).await;
    watch_module_events(client, watchers_state, specs_state).await?;

    Ok(())
}

async fn initialize_kube_client() -> Result<KubeClient, Box<dyn std::error::Error>> {
    Ok(KubeClient::try_default().await?)
}

async fn list_and_apply_modules(
    client: KubeClient,
    environment: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let available_modules = list_module(environment).await?;
    for module in available_modules {
        apply_module_kind(client.clone(), &module.manifest).await?;
        apply_module_crd(client.clone(), &module.manifest).await?;
    }
    Ok(())
}

async fn watch_module_events(
    client: KubeClient,
    watchers_state: Arc<Mutex<HashMap<String, ()>>>,
    specs_state: Arc<Mutex<HashMap<String, Value>>>,
) -> Result<(), Box<dyn std::error::Error>> {
    let modules_api: Api<Module> = Api::all(client.clone());
    let modules_watcher = watcher(modules_api, watcher::Config::default());

    modules_watcher
        .for_each(|event| async {
            info!("Received module event: {:?}", event);
            handle_module_watcher_event(
                event,
                client.clone(),
                watchers_state.clone(),
                specs_state.clone(),
            )
            .await;
        })
        .await;

    Ok(())
}

async fn handle_module_watcher_event(
    event: Result<watcher::Event<Module>, watcher::Error>,
    client: KubeClient,
    watchers_state: Arc<Mutex<HashMap<String, ()>>>,
    specs_state: Arc<Mutex<HashMap<String, Value>>>,
) {
    match event {
        Ok(watcher::Event::Deleted(module)) => {
            warn!("Deleted module: {}", module.spec.module_name);
            remove_module_watcher(client.clone(), module, watchers_state.clone()).await;
        }
        Ok(watcher::Event::Restarted(modules)) => {
            let module_names: String = modules
                .iter()
                .map(|m| m.spec.module_name.clone())
                .collect::<Vec<_>>()
                .join(",");
            warn!("Restarted modules: {}", module_names);
            for module in modules {
                add_module_watcher(
                    client.clone(),
                    module,
                    watchers_state.clone(),
                    specs_state.clone(),
                )
                .await;
            }
        }
        Ok(watcher::Event::Applied(module)) => {
            warn!("Applied module: {}", module.spec.module_name);
            add_module_watcher(
                client.clone(),
                module,
                watchers_state.clone(),
                specs_state.clone(),
            )
            .await;
        }
        Err(e) => {
            panic!("handle_module_watcher_event: {:?}", e)
        }
    }
}

async fn spawn_message_polling(specs_state: Arc<Mutex<HashMap<String, Value>>>) {
    tokio::spawn(async move {
        let queue_url = match create_queue_and_subscribe_to_topic(
            "arn:aws:sns:eu-central-1:053475148537:events-topic-eu-central-1-dev".to_string(),
        )
        .await
        {
            Ok(url) => url,
            Err(e) => {
                error!("Failed to create queue and subscribe to topic: {}", e);
                return;
            }
        };

        let handler: Arc<SqsMessageHandler> = Arc::new(|state, client, msg| {
            Box::pin(handle_event_message(state, client, msg)) // Wrap in Box::pin for Future
        });
        if let Err(e) = poll_sqs_messages(queue_url, specs_state, handler).await {
            error!("Failed to poll SQS messages: {}", e);
        }
    });
}

async fn handle_event_message(
    specs_state: Arc<Mutex<HashMap<String, serde_json::Value>>>,
    kube_client: kube::Client,
    message: Message,
) -> Result<(), Box<anyhow::Error>> {
    if let Some(body) = message.body() {
        if let Ok(outer_parsed) = serde_json::from_str::<Value>(body) {
            if let Some(inner_message_str) = outer_parsed.get("Message").and_then(|m| m.as_str()) {
                if let Ok(inner_parsed) = serde_json::from_str::<Value>(inner_message_str) {
                    if let Some(deployment_id) =
                        inner_parsed.get("deployment_id").and_then(|d| d.as_str())
                    {
                        info!("Deployment ID: {:?}", deployment_id);

                        warn!("Received message: {:?}", inner_parsed);

                        status_check(deployment_id.to_string(), specs_state, kube_client);
                    }
                }
            }
        }
    }
    Ok(())
}
