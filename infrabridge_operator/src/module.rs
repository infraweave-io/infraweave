use kube::Client as KubeClient;
use serde_json::Value;
use std::{collections::HashMap, sync::Arc};
use tokio::sync::Mutex;

use log::info;

use crate::{crd::Module, kind::watch_for_kind_changes};

pub async fn remove_module_watcher(
    _client: KubeClient,
    module: Module,
    watchers_state: Arc<Mutex<HashMap<String, ()>>>,
) {
    let kind = module.spec.module_name.clone();
    let mut watchers = watchers_state.lock().await;
    watchers.remove(&kind);
}

pub async fn add_module_watcher(
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
            watch_for_kind_changes(&client_clone, kind_clone, watchers_state_clone, specs_state)
                .await;
        });

        watchers.insert(kind, ());
    } else {
        info!("Watcher already exists for kind: {}", &kind);
    }
}
