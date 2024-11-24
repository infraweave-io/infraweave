// use chrono::DateTime;
// use chrono::Utc;
// use kube::Client as KubeClient;

// use log::{info, warn};

// use kube::api::DynamicObject;

// use crate::defs::{FINALIZER_NAME, KUBERNETES_GROUP};
// use crate::patch::patch_kind;

// use crate::utils::{
//     get_finalizers, get_name, has_deletion_finalizer, is_marked_for_deletion, set_finalizer,
// };

// pub async fn watch_for_kind_changes(
//     client: &KubeClient,
//     kind: String,
//     _watchers_state: Arc<Mutex<HashMap<String, ()>>>,
//     specs_state: Arc<Mutex<HashMap<String, Value>>>,
// ) {
//     let gvk = GroupVersionKind::gvk(KUBERNETES_GROUP, "v1", &kind);
//     let resource = ApiResource::from_gvk(&gvk);
//     let api: Api<DynamicObject> = Api::all_with(client.clone(), &resource);

//     warn!("Watching for changes on kind: {}", &kind);

//     let kind_watcher = watcher(api.clone(), watcher::Config::default());
//     kind_watcher
//         .for_each(|event| async {
//             println!("Event: {:?}", event.unwrap().clone());
//             // match event {
//             //     Ok(Event::Applied(crd)) => {
//             //         handle_applied_event(&client, crd, &kind, specs_state.clone()).await
//             //     }
//             //     Ok(Event::Restarted(crds)) => handle_restarted_event(&client, crds, &kind).await,
//             //     Ok(Event::Deleted(crd)) => handle_deleted_event(crd, &kind),
//             //     Err(ref e) => {
//             //         warn!("Event: {:?}", event);
//             //         warn!("Error: {:?}", e);
//             //     }
//             // }
//         })
//         .await;
// }

// async fn handle_applied_event(
//     client: &KubeClient,
//     crd: DynamicObject,
//     kind: &str,
//     specs_state: Arc<Mutex<HashMap<String, Value>>>,
// ) {
//     warn!("Event::Applied crd: {:?}", crd);
//     let annotations = get_annotations(&crd);
//     let deployment_id = get_deployment_id(&annotations);
//     let spec = get_spec(&crd);
//     let prev_spec = get_prev_spec(&deployment_id, specs_state.clone()).await;

//     let no_spec_change = prev_spec == spec && deployment_id != "";

//     if no_spec_change && !is_marked_for_deletion(&crd) {
//         warn!(
//             "No change in specs for: kind: {}, name: {}",
//             &kind,
//             crd.metadata.name.unwrap_or_else(|| "noname".to_string())
//         );
//         return; // Exit early
//     } else if is_marked_for_deletion(&crd) {
//         info!("Item is marked for deletion, checking if already sent destroy query");

//         if is_deleting(&deployment_id, specs_state.clone()).await {
//             warn!("Item is marked for deletion and already sent destroy query");
//             return;
//         }

//         // destroy_infra(&crd, kind, specs_state).await;
//         return; // Exit early
//     } else {
//         warn!(
//             "Specs changed for: kind: {}, name: {}",
//             &kind,
//             get_name(&crd),
//         );
//         warn!("New spec: {:?}", spec);
//         warn!("Previous spec: {:?}", prev_spec);
//     }

//     set_spec_for_deployment_id(&deployment_id, &spec, specs_state).await;

//     // Check resourceStatus as this determines current state of the resource
//     // and what action to take
//     let resource_status = get_resource_status(&crd);

//     // Get some data from the CRD
//     let name = get_name(&crd);

//     warn!("Annotations: {:?}", annotations);

//     info!("ResourceStatus: {}", resource_status);
//     match resource_status.as_str() {
//         // TODO: Use typed enum instead of string
//         "" => {
//             warn!(
//                 "Will mutate infra for: deployment_id: {}, kind: {}, name: {}",
//                 deployment_id, kind, name
//             );

//             // Dependencies are specified in annotation infraweave.io/dependsOn as a comma separated list with the format:
//             // <kind>::<name>,<kind>::<name>,...

//             // apiVersion: infraweave.io/v1
//             // kind: IAMRole
//             // metadata:
//             //   name: my-iam-role
//             //   namespace: default
//             //   annotations:
//             //     infraweave.io/dependsOn: S3Bucket::my-s3-bucket,Lambda::my-lambda-function,DynamoDB::my-dynamodb-table

//             if wait_on_dependencies(&client, &crd).await {
//                 warn!("Not all dependencies are ready, not creating {}", name);
//                 let module = kind;
//                 patch_waiting_on_dependencies(module, &name).await;
//                 return;
//             }

//             apply_infra(client, crd, kind).await;
//             // schedule_status_check(
//             //     5,
//             //     "S3Bucket-my-s3-bucket-c7q".to_string(),
//             // );
//         }
//         "Creating" => {
//             // Set up periodic checks for status

//             // let infra_status = get_infraweave_status(deployment_id.clone()).await;
//             // schedule_status_check(
//             //     5,
//             //     "S3Bucket-my-s3-bucket-c7q".to_string(),
//             // );
//         }
//         "Deployed" => {

//             // Setting status to deployed again to update the lastStatusUpdate time
//             // set_status_for_cr(
//             //     client.clone(),
//             //     kind.clone(),
//             //     name.clone(),
//             //     plural,
//             //     namespace,
//             //     "Deployed".to_string()
//             // ).await;

//             // schedule_status_check(
//             //     15,
//             //     "S3Bucket-my-s3-bucket-c7q".to_string(),
//             // );
//         }
//         _ => {
//             info!("ResourceStatus: {}", resource_status);
//         }
//     }
// }

// async fn handle_restarted_event(client: &KubeClient, crds: Vec<DynamicObject>, kind: &str) {
//     warn!("Event::Restarted crds: {:?}", crds);
//     for crd in crds {
//         let name = get_name(&crd);
//         info!("Restarted {}: {}, data: {:?}", &kind, name, crd.data);

//         if !is_marked_for_deletion(&crd) && !has_deletion_finalizer(&crd) {
//             warn!(
//                 "item is not marked for deletion and does not have a finalizer, setting finalizer"
//             );
//             set_finalizer(client, &crd, kind.to_string()).await;
//         }
//     }
// }

// fn handle_deleted_event(crd: DynamicObject, kind: &str) {
//     let name = get_name(&crd);
//     warn!("Event::Deleted {}: {}, data: {:?}", &kind, name, crd.data);

//     let finalizers = get_finalizers(&crd);
//     info!("Finalizers: {:?}", finalizers);
//     if is_marked_for_deletion(&crd) && finalizers.contains(&FINALIZER_NAME.to_string()) {
//         info!("item is marked for deletion and has finalizer");
//     }
// }

// async fn wait_on_dependencies(client: &KubeClient, crd: &DynamicObject) -> bool {
//     let dependencies = get_dependencies(&crd);
//     warn!("Dependencies: {:?}", dependencies);

//     let mut all_ready = true;
//     if dependencies.len() > 0 {
//         for dep in dependencies {
//             warn!("Checking dependency: {}", dep);
//             // Get the status of the dependency
//             let parts = dep.split("::").collect::<Vec<&str>>();
//             let (kind, name) = (parts[0], parts[1]);
//             let namespace = get_namespace(&crd);
//             let api = get_api_for_kind(&client, &namespace, &kind);
//             let resource = api.get(&name).await;
//             match resource {
//                 Ok(res) => {
//                     let resource_status = get_resource_status(&res);
//                     if resource_status != "apply: finished" {
//                         all_ready = false;
//                         warn!("Dependency {} is not ready", dep);
//                     }

//                     set_prerequisite_for(&api, kind, name, res).await;
//                 }
//                 Err(e) => {
//                     all_ready = false;
//                     warn!("Failed to fetch dependency: {}", e);
//                 }
//             }
//         }
//     }
//     !all_ready
// }

// pub async fn apply_infra(client: &KubeClient, crd: DynamicObject, kind: &str) {
//     let command = "apply".to_string();
//     let name = get_name(&crd);
//     let annotations = get_annotations(&crd);
//     let variables = get_spec(&crd)["variables"].clone();
//     let deployment_id = get_deployment_id(&annotations);
//     let environment = "dev".to_string();
//     let module = kind.to_string();
//     let namespace = get_namespace(&crd);
//     let plural = get_plural(kind);

//     info!("Applied {}: {}, crd.data: {:?}", &kind, name, crd.data);

//     // Assert deployment_id is "", otherwise this function is used incorrectly
//     assert_eq!(deployment_id, "");

//     // let new_deployment_id = match mutate_infra(
//     //     command,
//     //     module,
//     //     module_version.to_string(),
//     //     name.clone(),
//     //     environment,
//     //     deployment_id,
//     //     variables,
//     //     serde_json::json!(annotations),
//     // )
//     // .await
//     // {
//     //     Ok(id) => id,
//     //     Err(e) => {
//     //         error!("Failed to mutate infra: {}", e);
//     //         return;
//     //     }
//     // };

//     // // Get the current time in UTC
//     // let now: DateTime<Utc> = Utc::now();
//     // // Format the timestamp to RFC 3339 without microseconds
//     // let timestamp = now.format("%Y-%m-%dT%H:%M:%S%:z").to_string();

//     // patch_kind(
//     //     client.clone(),
//     //     new_deployment_id.to_string(),
//     //     kind.to_string(),
//     //     name,
//     //     plural,
//     //     namespace,
//     //     serde_json::json!({
//     //         "metadata": {
//     //             "annotations": {
//     //                 "deploymentId": new_deployment_id,
//     //             }
//     //         },
//     //         "status": {
//     //             "resourceStatus": "queried",
//     //             "lastStatusUpdate": timestamp,
//     //         }
//     //     }),
//     // )
//     // .await;
// }

// async fn patch_waiting_on_dependencies(module: &str, name: &str) {
//     // Get the current time in UTC
//     let now: DateTime<Utc> = Utc::now();
//     // Format the timestamp to RFC 3339 without microseconds
//     let timestamp = now.format("%Y-%m-%dT%H:%M:%S%:z").to_string();

//     patch_kind(
//         KubeClient::try_default().await.unwrap(),
//         "".to_string(),
//         module.to_string(),
//         name.to_string(),
//         module.to_lowercase() + "s",
//         "default".to_string(),
//         serde_json::json!({
//             "status": {
//                 "resourceStatus": "waiting for dependencies",
//                 "lastStatusUpdate": timestamp,
//             }
//         }),
//     )
//     .await;
// }

// async fn set_prerequisite_for(
//     api: &Api<DynamicObject>,
//     kind: &str,
//     name: &str,
//     res: DynamicObject,
// ) {
//     // Patch annotation of that resource to indicate we are relying on it
//     // This is to ensure that if the dependency is deleted, it will not be deleted
//     // until this is deleted
//     let prerequisite_for_key = &format!("{}/prerequisiteFor", KUBERNETES_GROUP);
//     let existing_relied_on_by = get_annotation_key(&res, prerequisite_for_key);

//     let updated_existing_relied_on_by = if existing_relied_on_by != "" {
//         format!("{},{}::{}", existing_relied_on_by, kind, name)
//     } else {
//         format!("{}::{}", kind, name)
//     };
//     let patch = serde_json::json!({
//         "metadata": {
//             "annotations": {
//                 prerequisite_for_key: updated_existing_relied_on_by,
//             }
//         }
//     });
//     let patch_params = kube::api::PatchParams::default();
//     match api
//         .patch(&name, &patch_params, &kube::api::Patch::Merge(&patch))
//         .await
//     {
//         Ok(_) => warn!(
//             "Successfully patched dependency: {}::{} with {}: {}",
//             kind, name, prerequisite_for_key, updated_existing_relied_on_by
//         ),
//         Err(e) => warn!(
//             "Failed to patch dependency: {}::{} with {}: {} due to {}. Maybe it doesn't exist yet?",
//             kind, name, prerequisite_for_key, updated_existing_relied_on_by, e
//         ),
//     }
// }
