// use kube::{Api, Client as KubeClient};
// use serde_json::Value;
// use std::{collections::HashMap, sync::Arc};
// use tokio::sync::Mutex;

// use kube::api::{ApiResource, DynamicObject, GroupVersionKind};

// use std::collections::BTreeMap;

// use crate::defs::{FINALIZER_NAME, KUBERNETES_GROUP};
// use crate::finalizer::get_deletion_key;
// use crate::patch::patch_kind;

// pub fn get_annotations(crd: &DynamicObject) -> BTreeMap<String, String> {
//     crd.metadata
//         .annotations
//         .clone()
//         .unwrap_or_else(BTreeMap::new)
// }

// pub fn get_annotation_key(crd: &DynamicObject, key: &str) -> String {
//     get_annotations(crd)
//         .get(key)
//         .map(|s| s.clone())
//         .unwrap_or("".to_string())
// }

// pub fn get_status(crd: &DynamicObject) -> BTreeMap<String, Value> {
//     crd.data
//         .get("status")
//         .and_then(|s| serde_json::from_value::<BTreeMap<String, Value>>(s.clone()).ok())
//         .unwrap_or_else(BTreeMap::new)
// }

// pub fn get_resource_status(crd: &DynamicObject) -> String {
//     get_status(&crd)
//         .get("resourceStatus")
//         .and_then(|v| v.as_str())
//         .unwrap_or("")
//         .to_string()
// }

// pub fn get_deployment_id(annotations: &BTreeMap<String, String>) -> String {
//     annotations
//         .get("deploymentId")
//         .map(|s| s.clone())
//         .unwrap_or("".to_string())
// }

// pub fn get_spec(crd: &DynamicObject) -> Value {
//     crd.data.get("spec").unwrap().clone()
// }

// pub async fn get_prev_spec(
//     deployment_id: &str,
//     specs_state: Arc<Mutex<HashMap<String, Value>>>,
// ) -> Value {
//     specs_state
//         .lock()
//         .await
//         .get(deployment_id)
//         .map(|v| v.clone())
//         .unwrap_or_else(|| serde_json::json!({}))
// }

// pub fn get_name(crd: &DynamicObject) -> String {
//     crd.metadata
//         .name
//         .clone()
//         .unwrap_or_else(|| "noname".to_string())
// }

// pub fn get_plural(kind: &str) -> String {
//     kind.to_lowercase() + "s"
// }

// pub async fn set_is_deleting(deployment_id: &str, specs_state: Arc<Mutex<HashMap<String, Value>>>) {
//     let deletion_key = get_deletion_key(deployment_id.to_string());
//     let deletion_json = serde_json::json!({
//         "deleting": "true"
//     });
//     specs_state
//         .lock()
//         .await
//         .insert(deletion_key.clone(), deletion_json.clone());
// }

// pub async fn is_deleting(
//     deployment_id: &str,
//     specs_state: Arc<Mutex<HashMap<String, Value>>>,
// ) -> bool {
//     let deletion_key = get_deletion_key(deployment_id.to_string());
//     specs_state
//         .lock()
//         .await
//         .get(&deletion_key)
//         .map(|v| v.get("deleting").map(|s| s == "true").unwrap_or(false))
//         .unwrap_or(false)
// }

// pub fn is_marked_for_deletion(crd: &DynamicObject) -> bool {
//     crd.metadata.deletion_timestamp.is_some()
// }

// pub fn get_namespace(crd: &DynamicObject) -> String {
//     crd.metadata
//         .namespace
//         .clone()
//         .unwrap_or_else(|| "default".to_string())
// }

// pub fn get_finalizers(crd: &DynamicObject) -> Vec<String> {
//     crd.metadata
//         .finalizers
//         .clone()
//         .unwrap_or_else(|| Vec::new())
// }

// pub fn has_deletion_finalizer(crd: &DynamicObject) -> bool {
//     let finalizers = get_finalizers(crd);
//     finalizers.contains(&FINALIZER_NAME.to_string())
// }

// pub async fn set_finalizer(client: &KubeClient, crd: &DynamicObject, kind: String) {
//     let name = get_name(crd);
//     let plural = get_plural(&kind);
//     let namespace = get_namespace(crd);
//     let deployment_id = get_deployment_id(&get_annotations(crd));
//     patch_kind(
//         client.clone(),
//         deployment_id.clone(),
//         kind.to_string(),
//         name.clone(),
//         plural,
//         namespace,
//         serde_json::json!({
//             "metadata": {
//                 "finalizers": [FINALIZER_NAME.to_string()]
//             }
//         }),
//     )
//     .await;
// }

// pub async fn set_spec_for_deployment_id(
//     deployment_id: &str,
//     spec: &Value,
//     specs_state: Arc<Mutex<HashMap<String, Value>>>,
// ) {
//     specs_state
//         .lock()
//         .await
//         .insert(deployment_id.to_string(), spec.clone());
// }

// pub fn get_dependencies(crd: &DynamicObject) -> Vec<String> {
//     let depends_on_key = &format!("{}/{}", KUBERNETES_GROUP, "dependsOn");
//     crd.metadata
//         .annotations
//         .as_ref()
//         .and_then(|annotations| annotations.get(depends_on_key))
//         .map_or_else(Vec::new, |dependencies_str| {
//             dependencies_str
//                 .split(',')
//                 .filter(|s| !s.is_empty())
//                 .map(|s| s.trim().to_string())
//                 .collect()
//         })
// }

// pub fn get_api_for_kind(client: &KubeClient, namespace: &str, kind: &str) -> Api<DynamicObject> {
//     let api_resource = ApiResource::from_gvk_with_plural(
//         &GroupVersionKind {
//             group: KUBERNETES_GROUP.into(),
//             version: "v1".into(),
//             kind: kind.to_string(),
//         },
//         &get_plural(kind),
//     );
//     let api: Api<DynamicObject> = Api::namespaced_with(client.clone(), &namespace, &api_resource);
//     api
// }
