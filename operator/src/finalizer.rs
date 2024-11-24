// use kube::api::Api;
// use kube::api::ApiResource;
// use kube::api::DynamicObject;
// use kube::api::GroupVersionKind;
// use kube::client::Client as KubeClient;
use serde_json::json;

// use serde_json::Value;
// use std::{collections::HashMap, sync::Arc};
// use tokio::sync::Mutex;

// use log::warn;

use crate::defs::FINALIZER_NAME;

#[allow(dead_code)]
pub fn ensure_finalizer(patch_json: serde_json::Value) -> serde_json::Value {
    let mut patch_json_with_timestamp = patch_json.clone();

    if patch_json_with_timestamp["metadata"].is_null() {
        patch_json_with_timestamp["metadata"] = json!({});
    }

    if patch_json_with_timestamp["metadata"]["finalizers"].is_null() {
        patch_json_with_timestamp["metadata"]["finalizers"] = json!([]);
    }

    let existing_finalizers = patch_json_with_timestamp["metadata"]["finalizers"]
        .as_array_mut() // Get a mutable reference to the array
        .unwrap(); // Safe unwrap because we just ensured it exists

    if !existing_finalizers
        .iter()
        .any(|f| f.as_str() == Some(FINALIZER_NAME))
    {
        existing_finalizers.push(json!(FINALIZER_NAME));
    }

    patch_json_with_timestamp
}

// pub async fn delete_kind_finalizer(
//     client: KubeClient,
//     kind: String,
//     name: String,
//     plural: String,
//     namespace: String,
//     specs_state: Arc<Mutex<HashMap<String, Value>>>,
//     deployment_id: String,
// ) {
//     warn!(
//         "Deleting kind finalizer for: kind: {}, name: {}, plural: {}, namespace: {}",
//         &kind, name, plural, namespace
//     );
//     let api_resource = ApiResource::from_gvk_with_plural(
//         &GroupVersionKind {
//             group: KUBERNETES_GROUP.into(),
//             version: "v1".into(),
//             kind: kind.clone(),
//         },
//         &plural,
//     );
//     let api: Api<DynamicObject> = Api::namespaced_with(client, &namespace, &api_resource);

//     // Remove the deployment_id from the specs_state
//     specs_state.lock().await.remove(&deployment_id);
//     let deletion_key = get_deletion_key(deployment_id.clone());
//     specs_state.lock().await.remove(&deletion_key);

//     let resource = api.get(&name).await;
//     match resource {
//         Ok(res) => {
//             let finalizers = res.metadata.finalizers.unwrap_or_default();
//             let finalizers_to_keep: Vec<String> = finalizers
//                 .into_iter()
//                 .filter(|f| f != FINALIZER_NAME)
//                 .collect();

//             warn!(
//                 "Finalizers after removing {}: {:?}",
//                 FINALIZER_NAME, finalizers_to_keep
//             );

//             let patch = serde_json::json!({
//                 "metadata": {
//                     "finalizers": finalizers_to_keep,
//                     "resourceVersion": res.metadata.resource_version,
//                 }
//             });

//             let params = kube::api::PatchParams::default();
//             match api
//                 .patch(&name, &params, &kube::api::Patch::Merge(&patch))
//                 .await
//             {
//                 Ok(_) => warn!(
//                     "Finalizer removed for: kind: {}, name: {}, plural: {}, namespace: {}",
//                     &kind, name, plural, namespace
//                 ),
//                 Err(e) => warn!("Error deleting finalizer: {}", e),
//             }
//         }
//         Err(e) => warn!("Error fetching resource: {}", e),
//     }
// }

// pub fn get_deletion_key(deployment_id: String) -> String {
//     format!("{}-{}", deployment_id, "deleting")
// }
