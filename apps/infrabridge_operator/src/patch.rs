
use kube::api::{Api, Patch, PatchParams};
use kube::client::Client as KubeClient;
use kube::api::DynamicObject;
use kube::api::ApiResource;
use kube::api::GroupVersionKind;
use serde_json::json;
use log::{error, info, warn};

use crate::FINALIZER_NAME;

pub async fn patch_kind(
    client: KubeClient,
    deployment_id: String,
    kind: String,
    name: String,
    plural: String,
    namespace: String,
    patch_json: serde_json::Value,
) {
    warn!("patch_kind called for: kind: {}, name: {}, plural: {}, namespace: {}, patch_json: {:?}", &kind, name, plural, namespace, patch_json);
    let api_resource = ApiResource::from_gvk_with_plural(
        &GroupVersionKind {
            group: "infrabridge.io".into(),
            version: "v1".into(),
            kind: kind.clone(),
        }, 
        &plural
    );
    let api: Api<DynamicObject> = Api::namespaced_with(client, &namespace, &api_resource);

    // info!("Attempting to patch CR with GVK: {:?}, name: {}, namespace: {}", gvk, name, namespace);

    let crd = match api.get(&name).await {
        Ok(crd) => crd,
        Err(e) => {
            error!("Failed to get CR for: {}: {:?}. Does it not exist?", name, e);
            return;
        }
    };

    let fetched_deployment_id = crd.metadata.annotations.as_ref().and_then(|annotations| annotations.get("deploymentId")).map(|s| s.as_str()).unwrap_or("");

    if fetched_deployment_id != "" && fetched_deployment_id != &deployment_id {
        error!("Deployment ID mismatch for: {}. Expected: {}, Got: {}", name, deployment_id, fetched_deployment_id);
        return;
    }

    let patch_json_finalizer = ensure_finalizer(patch_json);

    let patch = Patch::Merge(&patch_json_finalizer);
    let patch_params = PatchParams::default();

    warn!("Patching CR with: {:?}", patch);

    match api.patch(&name, &patch_params, &patch).await {
        Ok(_) => info!("Successfully updated CR status for: {}", name),
        Err(e) => {   
            error!("Failed to update CR status for: {}: {:?} altough it exists", name, e)
        }
    }
}

fn ensure_finalizer(patch_json: serde_json::Value) -> serde_json::Value {
    let mut patch_json_with_timestamp = patch_json.clone();

    // Your finalizer name as a string.
    let finalizer_name_str: &str = FINALIZER_NAME;

    // Check if "metadata" exists, if not create it.
    if patch_json_with_timestamp["metadata"].is_null() {
        patch_json_with_timestamp["metadata"] = json!({});
    }

    // Check if "finalizers" exists within "metadata", if not create it as an empty array.
    if patch_json_with_timestamp["metadata"]["finalizers"].is_null() {
        patch_json_with_timestamp["metadata"]["finalizers"] = json!([]);
    }

    // Assuming the finalizers are stored as an array of strings.
    let existing_finalizers = patch_json_with_timestamp["metadata"]["finalizers"]
        .as_array_mut() // Get a mutable reference to the array
        .unwrap(); // Safe unwrap because we just ensured it exists

    // Check if our FINALIZER_NAME is not already in the list, and if so, append it.
    if !existing_finalizers.iter().any(|f| f.as_str() == Some(finalizer_name_str)) {
        existing_finalizers.push(json!(finalizer_name_str));
    }

    patch_json_with_timestamp
}