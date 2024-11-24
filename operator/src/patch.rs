// use kube::api::ApiResource;
// use kube::api::DynamicObject;
// use kube::api::GroupVersionKind;
// use kube::api::{Api, Patch, PatchParams};
// use kube::client::Client as KubeClient;
// use log::{error, info, warn};

// use crate::defs::KUBERNETES_GROUP;
// use crate::finalizer::ensure_finalizer;

// pub async fn patch_kind(
//     client: KubeClient,
//     deployment_id: String,
//     kind: String,
//     name: String,
//     plural: String,
//     namespace: String,
//     patch_json: serde_json::Value,
// ) {
//     warn!(
//         "patch_kind called for: kind: {}, name: {}, plural: {}, namespace: {}, patch_json: {:?}",
//         &kind, name, plural, namespace, patch_json
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

//     // info!("Attempting to patch CR with GVK: {:?}, name: {}, namespace: {}", gvk, name, namespace);

//     let crd = match api.get(&name).await {
//         Ok(crd) => crd,
//         Err(e) => {
//             error!(
//                 "Failed to get CR for: {}: {:?}. Does it not exist?",
//                 name, e
//             );
//             return;
//         }
//     };

//     let fetched_deployment_id = crd
//         .metadata
//         .annotations
//         .as_ref()
//         .and_then(|annotations| annotations.get("deploymentId"))
//         .map(|s| s.as_str())
//         .unwrap_or("");

//     if fetched_deployment_id != "" && fetched_deployment_id != &deployment_id {
//         error!(
//             "Deployment ID mismatch for: {}. Expected: {}, Got: {}",
//             name, deployment_id, fetched_deployment_id
//         );
//         return;
//     }

//     let patch_json_finalizer = if fetched_deployment_id != "" {
//         ensure_finalizer(patch_json)
//     } else {
//         patch_json
//     };

//     let patch = Patch::Merge(&patch_json_finalizer);
//     let patch_params = PatchParams::default();

//     warn!("Patching CR with: {:?}", patch);

//     match api.patch(&name, &patch_params, &patch).await {
//         Ok(_) => info!("Successfully updated CR status for: {}", name),
//         Err(e) => {
//             error!(
//                 "Failed to update CR status for: {}: {:?} altough it exists",
//                 name, e
//             )
//         }
//     }
// }
