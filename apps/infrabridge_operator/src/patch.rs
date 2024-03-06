
use kube::api::{Api, Patch, PatchParams};
use kube::client::Client as KubeClient;
use kube::api::DynamicObject;
use kube::api::ApiResource;
use kube::api::GroupVersionKind;
use chrono::{DateTime, Utc};
use serde_json::json;
use log::{info, error, debug};

pub async fn set_status_for_cr(
    client: KubeClient,
    kind: String,
    name: String,
    plural: String,
    namespace: String,
    status: String,
) {
    debug!("Setting status for: kind: {}, name: {}, plural: {}, namespace: {}, status: {}", &kind, name, plural, namespace, status);
    let api_resource = ApiResource::from_gvk_with_plural(
        &GroupVersionKind {
            group: "infrabridge.io".into(),
            version: "v1".into(),
            kind: kind.clone(),
        }, 
        &plural
    );
    let api: Api<DynamicObject> = Api::namespaced_with(client, &namespace, &api_resource);

    // Get the current time in UTC
    let now: DateTime<Utc> = Utc::now();
    // Format the timestamp to RFC 3339 without microseconds
    let timestamp = now.format("%Y-%m-%dT%H:%M:%S%:z").to_string();

    let patch_json = json!({
        "status": {
            "resourceStatus": status,
            "lastStatusUpdate": timestamp,
        }
    });

    let patch = Patch::Merge(&patch_json);
    let patch_params = PatchParams::default();


    info!("Patch being applied: {:?} for {}: {}", &patch_json, &kind, &name);
    // info!("Attempting to patch CR with GVK: {:?}, name: {}, namespace: {}", gvk, name, namespace);

    match api.patch(&name, &patch_params, &patch).await {
        Ok(_) => info!("Successfully updated CR status for: {}", name),
        Err(e) => error!("Failed to update CR status for: {}: {:?}", name, e),
    }
}