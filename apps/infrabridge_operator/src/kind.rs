use chrono::DateTime;
use chrono::Utc;
use kube::{api::Api, runtime::watcher, Client as KubeClient};
use kube_runtime::watcher::Event;
use log::error;
use serde_json::Value;
use std::{collections::HashMap, sync::Arc};
use tokio::sync::Mutex;

use log::{info, warn};

use futures::stream::StreamExt;
use kube::api::ApiResource;
use kube::api::DynamicObject;
use kube::api::GroupVersionKind;

use std::collections::BTreeMap;

use crate::defs::{FINALIZER_NAME, KUBERNETES_GROUP};
use crate::finalizer::get_deletion_key;
use crate::other::initiate_infra_setup;
use crate::patch::patch_kind;
use env_aws::mutate_infra;

pub async fn watch_for_kind_changes(
    client: &KubeClient,
    kind: String,
    _watchers_state: Arc<Mutex<HashMap<String, ()>>>,
    specs_state: Arc<Mutex<HashMap<String, Value>>>,
) {
    let gvk = GroupVersionKind::gvk(KUBERNETES_GROUP, "v1", &kind);
    let resource = ApiResource::from_gvk(&gvk);
    let api: Api<DynamicObject> = Api::all_with(client.clone(), &resource);

    warn!("Watching for changes on kind: {}", &kind);

    let kind_watcher = watcher(api.clone(), watcher::Config::default());
    kind_watcher
        .for_each(|event| async {
            match event {
                Ok(Event::Applied(crd)) => {
                    handle_applied_event(&client, crd, &kind, specs_state.clone()).await
                }
                Ok(Event::Restarted(crds)) => handle_restarted_event(&client, crds, &kind).await,
                Ok(Event::Deleted(crd)) => handle_deleted_event(crd, &kind),
                Err(ref e) => {
                    warn!("Event: {:?}", event);
                    warn!("Error: {:?}", e);
                }
            }
        })
        .await;
}

async fn handle_applied_event(
    client: &KubeClient,
    crd: DynamicObject,
    kind: &str,
    specs_state: Arc<Mutex<HashMap<String, Value>>>,
) {
    warn!("Event::Applied crd: {:?}", crd);
    let annotations = crd
        .metadata
        .annotations
        .clone()
        .unwrap_or_else(|| BTreeMap::new());
    let status = crd
        .data
        .get("status")
        .and_then(|s| serde_json::from_value::<BTreeMap<String, serde_json::Value>>(s.clone()).ok())
        .unwrap_or_else(|| BTreeMap::new());

    let spec = crd.data.get("spec").unwrap();
    let deployment_id = annotations
        .get("deploymentId")
        .map(|s| s.clone()) // Clone the string if found
        .unwrap_or("".to_string()); // Provide an owned empty String as the default
    let prev_spec = specs_state
        .lock()
        .await
        .get(&deployment_id)
        .map(|v| v.clone())
        .unwrap_or_else(|| serde_json::json!({}));

    let no_spec_change = &prev_spec == spec && deployment_id != "";

    if no_spec_change && !crd.metadata.deletion_timestamp.is_some() {
        warn!(
            "No change in specs for: kind: {}, name: {}",
            &kind,
            crd.metadata.name.unwrap_or_else(|| "noname".to_string())
        );
        return;
    } else if crd.metadata.deletion_timestamp.is_some() {
        info!("Item is marked for deletion, checking if already sent destroy query");

        let deletion_key = get_deletion_key(deployment_id.clone());
        let deletion_json = specs_state
            .lock()
            .await
            .get(&deletion_key)
            .map(|v| v.clone())
            .unwrap_or_else(|| serde_json::json!({}));

        if deletion_json
            .get("deleting")
            .map(|v| v == "true")
            .unwrap_or(false)
        {
            warn!("Item is marked for deletion and already sent destroy query");
            return;
        }

        let event = "destroy".to_string();
        let deployment_id = crd
            .metadata
            .annotations
            .as_ref()
            .and_then(|annotations| annotations.get("deploymentId").map(|s| s.clone())) // Clone the string if found
            .unwrap_or("".to_string()); // Provide an owned empty String as the default
        let spec = crd.data.get("spec").unwrap();
        let annotations = crd.metadata.annotations.unwrap_or_else(|| BTreeMap::new());
        // Convert `BTreeMap<String, String>` to `serde_json::Value` using `.into()`
        let annotations_value = serde_json::json!(annotations);
        let name = crd.metadata.name.unwrap_or_else(|| "noname".to_string());
        // Convert `BTreeMap<String, String>` to `serde_json::Value` using `.into()`
        let annotations_value = serde_json::json!(annotations);
        let plural = kind.to_lowercase() + "s"; // pluralize, this is a aligned in the crd-generator
                                                // let namespace = crd.metadata.namespace.unwrap_or_else(|| "default".to_string());

        warn!("MUTATE_INFRA inside deletion");
        let deployment_id = match mutate_infra(
            event,
            kind.to_string(),
            name.clone(),
            "dev".to_string(),
            deployment_id,
            spec.clone(),
            annotations_value,
        )
        .await
        {
            Ok(id) => id,
            Err(e) => {
                error!("Failed to mutate infra: {}", e);
                return;
            }
        };

        let module = kind;
        // Get the current time in UTC
        let now: DateTime<Utc> = Utc::now();
        // Format the timestamp to RFC 3339 without microseconds
        let timestamp = now.format("%Y-%m-%dT%H:%M:%S%:z").to_string();

        patch_kind(
            KubeClient::try_default().await.unwrap(),
            deployment_id.to_string(),
            module.to_string(),
            name.clone(),
            module.to_lowercase() + "s",
            "default".to_string(),
            serde_json::json!({
                "metadata": {
                    "annotations": {
                        "deploymentId": deployment_id,
                    }
                },
                "status": {
                    "resourceStatus": "queried",
                    "lastStatusUpdate": timestamp,
                }
            }),
        )
        .await;

        let deletion_json = serde_json::json!({
            "deleting": "true"
        });
        specs_state
            .lock()
            .await
            .insert(deletion_key.clone(), deletion_json.clone());

        return;
    } else {
        warn!("Current spec: {:?}", spec);
        warn!("Previous spec: {:?}", prev_spec);
    }
    specs_state
        .lock()
        .await
        .insert(deployment_id.clone(), spec.clone());

    // Check resourceStatus as this determines current state of the resource
    // and what action to take
    let resource_status = status
        .get("resourceStatus")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    // Get some data from the CRD
    let name = crd.metadata.name.unwrap_or_else(|| "noname".to_string());
    info!("Applied {}: {}, data: {:?}", &kind, name, crd.data);
    let event = "apply".to_string();
    let annotations_value = serde_json::json!(annotations);
    // let plural = kind.to_lowercase() + "s"; // pluralize, this is a aligned in the crd-generator
    // let namespace = crd.metadata.namespace.unwrap_or_else(|| "default".to_string());

    warn!("Annotations: {:?}", annotations);
    // let is_deleting_annotation_present = annotations.get("deleting").map(|s| s == "true").unwrap_or(false);

    info!("ResourceStatus: {}", resource_status);
    match resource_status {
        // TODO: Use typed enum instead of string
        "" => {
            warn!(
                "Will mutate infra for: deployment_id: {}, kind: {}, name: {}",
                deployment_id, kind, name
            );

            // Dependencies are specified in annotation infrabridge.io/dependsOn as a comma separated list with the format:
            // <kind>::<name>

            // apiVersion: infrabridge.io/v1
            // kind: IAMRole
            // metadata:
            //   name: my-iam-role
            //   namespace: default
            //   annotations:
            //     infrabridge.io/dependsOn: S3Bucket::my-s3-bucket,Lambda::my-lambda-function,DynamoDB::my-dynamodb-table

            let dependencies_str = crd
                .metadata
                .annotations
                .as_ref()
                .and_then(|annotations| {
                    annotations
                        .get("infrabridge.io/dependsOn")
                        .map(|s| s.clone())
                }) // Clone the string if found
                .unwrap_or("".to_string()); // Provide an owned empty String as the default

            let dependencies: Vec<String> =
                match dependencies_str.split(",").map(|s| s.to_string()).collect() {
                    many if many != vec![""] => many,
                    _ => vec![],
                };

            warn!("Dependencies: {:?}", dependencies);

            // Check that all dependencies in the same namespace are ready
            // If not, return
            if dependencies.len() > 0 {
                let mut all_ready = true;
                for dep in dependencies {
                    warn!("Checking dependency: {}", dep);
                    // Get the status of the dependency
                    let kind = dep.split("::").collect::<Vec<&str>>()[0];
                    let name = dep.split("::").collect::<Vec<&str>>()[1];
                    let namespace = crd
                        .data
                        .get("metadata")
                        .and_then(|m| m.get("namespace"))
                        .and_then(|n| n.as_str())
                        .unwrap_or("default");
                    let api_resource = ApiResource::from_gvk_with_plural(
                        &GroupVersionKind {
                            group: KUBERNETES_GROUP.into(),
                            version: "v1".into(),
                            kind: kind.to_string(),
                        },
                        &(kind.to_lowercase() + "s"),
                    );
                    let api: Api<DynamicObject> =
                        Api::namespaced_with(client.clone(), &namespace, &api_resource);
                    let resource = api.get(&name).await;
                    match resource {
                        Ok(res) => {
                            let status = res
                                .data
                                .get("status")
                                .and_then(|s| {
                                    serde_json::from_value::<BTreeMap<String, serde_json::Value>>(
                                        s.clone(),
                                    )
                                    .ok()
                                })
                                .unwrap_or_else(|| BTreeMap::new());
                            warn!("Status: {:?}", status);
                            let resource_status = status
                                .get("resourceStatus")
                                .and_then(|v| v.as_str())
                                .unwrap_or("");
                            if resource_status != "apply: finished" {
                                all_ready = false;
                                warn!("Dependency {} is not ready", dep);
                            }

                            // Patch annotation of that resource to indicate we are relying on it
                            // This is to ensure that if the dependency is deleted, it will not be deleted
                            // until this is deleted
                            let annotations = res
                                .metadata
                                .annotations
                                .clone()
                                .unwrap_or_else(|| BTreeMap::new());
                            let existing_relied_on_by = annotations
                                .get("infrabridge.io/prerequisiteFor")
                                .map(|s| s.clone())
                                .unwrap_or("".to_string());
                            let updated_existing_relied_on_by = if existing_relied_on_by != "" {
                                format!("{},{}::{}", existing_relied_on_by, kind, name)
                            } else {
                                format!("{}::{}", kind, name)
                            };
                            let patch = serde_json::json!({
                                "metadata": {
                                    "annotations": {
                                        "infrabridge.io/prerequisiteFor": updated_existing_relied_on_by,
                                    }
                                }
                            });
                            let patch_params = kube::api::PatchParams::default();
                            match api.patch(&name, &patch_params, &kube::api::Patch::Merge(&patch)).await {
                                Ok(_) => warn!("Successfully patched dependency: {} with infrabridge.io/prerequisiteFor: {}", dep, updated_existing_relied_on_by),
                                Err(e) => warn!("Failed to patch dependency: {} with infrabridge.io/prerequisiteFor: {} due to {}. Maybe it doesn't exist yet?", dep, updated_existing_relied_on_by, e),
                            }
                        }
                        Err(e) => {
                            all_ready = false;
                            warn!("Failed to fetch dependency: {}", e);
                        }
                    }
                }
                if !all_ready {
                    warn!("Not all dependencies are ready, not creating {}", name);
                    let module = kind;
                    // Get the current time in UTC
                    let now: DateTime<Utc> = Utc::now();
                    // Format the timestamp to RFC 3339 without microseconds
                    let timestamp = now.format("%Y-%m-%dT%H:%M:%S%:z").to_string();
                    patch_kind(
                        KubeClient::try_default().await.unwrap(),
                        "".to_string(),
                        module.to_string(),
                        name.clone(),
                        module.to_lowercase() + "s",
                        "default".to_string(),
                        serde_json::json!({
                            "status": {
                                "resourceStatus": "waiting for dependencies",
                                "lastStatusUpdate": timestamp,
                            }
                        }),
                    )
                    .await;
                    return;
                }
                warn!("All dependencies are ready, creating {}", name);
            } else {
                warn!("No dependencies for {}", name);
            }

            initiate_infra_setup(
                client.clone(),
                event,
                kind.to_string(),
                name.clone(),
                "dev".to_string(),
                deployment_id.clone(),
                spec.clone(),
                annotations_value.clone(),
            )
            .await;

            // schedule_status_check(
            //     5,
            //     "S3Bucket-my-s3-bucket-c7q".to_string(),
            // );
        }
        "Creating" => {
            // Set up periodic checks for status

            // let infra_status = get_infrabridge_status(deployment_id.clone()).await;
            // schedule_status_check(
            //     5,
            //     "S3Bucket-my-s3-bucket-c7q".to_string(),
            // );
        }
        "Deployed" => {

            // Setting status to deployed again to update the lastStatusUpdate time
            // set_status_for_cr(
            //     client.clone(),
            //     kind.clone(),
            //     name.clone(),
            //     plural,
            //     namespace,
            //     "Deployed".to_string()
            // ).await;

            // schedule_status_check(
            //     15,
            //     "S3Bucket-my-s3-bucket-c7q".to_string(),
            // );
        }
        _ => {
            info!("ResourceStatus: {}", resource_status);
        }
    }
}

async fn handle_restarted_event(client: &KubeClient, crds: Vec<DynamicObject>, kind: &str) {
    warn!("Event::Restarted crds: {:?}", crds);
    for crd in crds {
        let name = crd.metadata.name.unwrap_or_else(|| "noname".to_string());
        info!("Restarted {}: {}, data: {:?}", &kind, name, crd.data);

        let plural = kind.to_lowercase() + "s"; // pluralize, this is a aligned in the crd-generator
        let namespace = crd
            .metadata
            .namespace
            .unwrap_or_else(|| "default".to_string());
        let deployment_id = crd
            .metadata
            .annotations
            .as_ref()
            .and_then(|annotations| annotations.get("deploymentId").map(|s| s.clone())) // Clone the string if found
            .unwrap_or("".to_string()); // Provide an owned empty String as the default

        if !crd.metadata.deletion_timestamp.is_some()
            && !crd
                .metadata
                .finalizers
                .as_ref()
                .map(|f| f.contains(&FINALIZER_NAME.to_string()))
                .unwrap_or(false)
        {
            warn!("item is not marked for deletion, ensuring finalizer is set");

            patch_kind(
                client.clone(),
                deployment_id.clone(),
                kind.to_string(),
                name.clone(),
                plural,
                namespace,
                serde_json::json!({
                    "metadata": {
                        "finalizers": [FINALIZER_NAME.to_string()]
                    }
                }),
            )
            .await;
        }
    }
}

fn handle_deleted_event(crd: DynamicObject, kind: &str) {
    let name = crd.metadata.name.unwrap_or_else(|| "noname".to_string());
    warn!("Event::Deleted {}: {}, data: {:?}", &kind, name, crd.data);

    let finalizers = crd
        .metadata
        .finalizers
        .clone()
        .unwrap_or_else(|| Vec::new());
    info!("Finalizers: {:?}", finalizers);
    if crd.metadata.deletion_timestamp.is_some() && finalizers.contains(&FINALIZER_NAME.to_string())
    {
        info!("item is marked for deletion and has finalizer");
    }
}
