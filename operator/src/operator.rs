use anyhow;
use env_common::interface::GenericCloudHandler;
use env_common::logic::{is_deployment_in_progress, run_claim};
use env_defs::{CloudProvider, CloudProviderCommon, DeploymentResp, ExtraData, ModuleResp};
use env_utils::{epoch_to_timestamp, get_timestamp, indent};
use k8s_openapi::apiextensions_apiserver::pkg::apis::apiextensions::v1::CustomResourceDefinition;
use kube::api::{ApiResource, DynamicObject, PostParams};
use kube::{api::Api, runtime::watcher, Client as KubeClient};
use kube_leader_election::{LeaseLock, LeaseLockParams};
use std::collections::BTreeMap;
use std::env;
use std::time::Duration;
use tokio::time;

use futures::stream::StreamExt;

use crate::apply::apply_module_crd;
use crate::defs::{FINALIZER_NAME, KUBERNETES_GROUP, NAMESPACE, OPERATOR_NAME};

use kube::api::{Patch, PatchParams, ResourceExt};
use serde_json::json;

pub async fn start_operator(handler: &GenericCloudHandler) -> anyhow::Result<()> {
    let client: KubeClient = initialize_kube_client().await?;
    let leadership = create_lease_lock(client.clone());

    let mut watcher_started = false;

    loop {
        if acquire_leadership_and_run_once(handler, &leadership, &client, &mut watcher_started)
            .await
        {
            renew_leadership(&leadership).await;
        } else {
            println!("There is already a leader, waiting for it to release leadership");
            time::sleep(Duration::from_secs(15)).await;
        }
    }
}

fn create_lease_lock(client: KubeClient) -> LeaseLock {
    LeaseLock::new(
        client,
        NAMESPACE,
        LeaseLockParams {
            holder_id: get_holder_id(),
            lease_name: format!("{}-lock", OPERATOR_NAME),
            lease_ttl: Duration::from_secs(25),
        },
    )
}

fn get_holder_id() -> String {
    let pod_name = std::env::var("POD_NAME").unwrap_or_else(|_| "NO_POD_NAME_FOUND".into());
    format!("{}-{}", OPERATOR_NAME, pod_name)
}

async fn acquire_leadership_and_run_once(
    handler: &GenericCloudHandler,
    leadership: &LeaseLock,
    client: &KubeClient,
    watcher_started: &mut bool,
) -> bool {
    let lease = leadership.try_acquire_or_renew().await.unwrap();

    if lease.acquired_lease {
        println!("Leadership acquired!");
        list_and_apply_modules(handler, client.clone())
            .await
            .unwrap();

        if !*watcher_started {
            start_infraweave_watcher(handler, client.clone());
            *watcher_started = true;
        }

        return true;
    }
    false
}

async fn renew_leadership(leadership: &LeaseLock) {
    let mut renew_interval = time::interval(Duration::from_secs(10));

    loop {
        renew_interval.tick().await;
        if let Err(e) = leadership.try_acquire_or_renew().await {
            eprintln!("Lost leadership due to error: {:?}", e);
            break; // Exit if lease renewal fails
        } else {
            println!("Leadership renewed for {}", OPERATOR_NAME);
        }
    }
}

fn get_api_resource(kind: &str) -> ApiResource {
    ApiResource {
        api_version: format!("{}/v1", KUBERNETES_GROUP),
        group: KUBERNETES_GROUP.to_string(),
        version: "v1".to_string(),
        kind: kind.to_string(),
        plural: (kind.to_lowercase() + "s").to_string(),
    }
}

async fn initialize_kube_client() -> anyhow::Result<KubeClient> {
    Ok(KubeClient::try_default().await?)
}

async fn handle_resource_apply(
    handler: &GenericCloudHandler,
    client: &kube::Client,
    kind: &str,
    resource: DynamicObject,
    cluster_id: &str,
) -> anyhow::Result<()> {
    let api_resource = get_api_resource(kind);
    let namespace = resource
        .namespace()
        .unwrap_or_else(|| "default".to_string());
    let environment = format!("k8s-{}/{}", cluster_id, namespace);

    println!("Resource applied: {} - {:?}", kind, resource.metadata.name);

    if resource.metadata.deletion_timestamp.is_none() {
        if !resource.finalizers().contains(&FINALIZER_NAME.to_string()) {
            add_finalizer(client, &resource, &api_resource, &namespace).await?;
        } else {
            if should_reconcile(&resource) {
                reconcile_resource(
                    handler,
                    client,
                    &resource,
                    &api_resource,
                    kind,
                    &environment,
                )
                .await?;
            } else {
                println!(
                    "Generation unchanged, skipping reconciliation for {:?}",
                    resource.metadata.name
                );
            }
        }
    } else {
        if resource.finalizers().contains(&FINALIZER_NAME.to_string()) {
            handle_resource_deletion(
                handler,
                client,
                &resource,
                &api_resource,
                kind,
                &environment,
            )
            .await?;
        }
    }

    Ok(())
}

async fn add_finalizer(
    client: &kube::Client,
    resource: &DynamicObject,
    api_resource: &ApiResource,
    namespace: &str,
) -> anyhow::Result<()> {
    let patch_params = PatchParams::default();
    let patch = json!({
        "metadata": {
            "finalizers": [FINALIZER_NAME]
        }
    });
    let namespaced_api =
        Api::<DynamicObject>::namespaced_with(client.clone(), namespace, api_resource);
    namespaced_api
        .patch(
            &resource.metadata.name.clone().unwrap(),
            &patch_params,
            &Patch::Merge(&patch),
        )
        .await?;
    println!(
        "Added finalizer to {:?}",
        resource.metadata.name.as_ref().unwrap()
    );
    Ok(())
}

fn should_reconcile(resource: &DynamicObject) -> bool {
    if let Some(status) = resource.data.get("status") {
        let observed_generation = status
            .get("lastGeneration")
            .and_then(|g| g.as_i64())
            .unwrap_or(0);
        let metadata_generation = resource.metadata.generation.unwrap_or(0);

        if observed_generation == metadata_generation {
            return false;
        }

        println!(
            "Generation has changed from {} to {}; reconciling",
            observed_generation, metadata_generation
        );
    }
    true
}

async fn reconcile_resource(
    handler: &GenericCloudHandler,
    client: &kube::Client,
    resource: &DynamicObject,
    api_resource: &ApiResource,
    kind: &str,
    environment: &str,
) -> anyhow::Result<()> {
    let yaml = serde_yaml::to_value(&resource)?;
    println!("Applying {} manifest \n{:?}", kind, resource);

    let flags = vec![];
    let reference_fallback = "";
    let (job_id, deployment_id) = match run_claim(
        handler,
        &yaml,
        environment,
        "apply",
        flags,
        ExtraData::None,
        reference_fallback,
    )
    .await
    {
        Ok((job_id, deployment_id, _)) => {
            println!("Successfully applied {} manifest", kind);
            (job_id, deployment_id)
        }
        Err(e) => {
            return Err(anyhow::anyhow!(
                "Failed to apply {} manifest: {:?}",
                kind,
                e
            ));
        }
    };

    follow_job_until_finished(
        handler,
        client.clone(),
        resource,
        api_resource,
        job_id.as_str(),
        deployment_id.as_str(),
        environment,
        "Apply",
        "APPLY",
    )
    .await?;

    Ok(())
}

async fn handle_resource_deletion(
    handler: &GenericCloudHandler,
    client: &kube::Client,
    resource: &DynamicObject,
    api_resource: &ApiResource,
    kind: &str,
    environment: &str,
) -> anyhow::Result<()> {
    let namespace = resource
        .namespace()
        .unwrap_or_else(|| "default".to_string());

    // Perform cleanup before deletion
    let yaml = serde_yaml::to_value(&resource)?;
    println!("Deleting {} manifest \n{:?}", kind, resource);

    let flags = vec![];
    let reference_fallback = "";
    let (job_id, deployment_id) = match run_claim(
        handler,
        &yaml,
        environment,
        "destroy",
        flags,
        ExtraData::None,
        reference_fallback,
    )
    .await
    {
        Ok((job_id, deployment_id, _)) => {
            println!("Successfully requested destroying {} manifest", kind);
            update_resource_status(
                client.clone(),
                resource,
                api_resource,
                "Deleted requested",
                get_timestamp().as_str(),
                "Resource deletetion requested successfully",
                &job_id,
            )
            .await?;
            (job_id, deployment_id)
        }
        Err(e) => {
            return Err(anyhow::anyhow!(
                "Failed to request destroying {} manifest: {:?}",
                kind,
                e
            ));
        }
    };

    follow_job_until_finished(
        handler,
        client.clone(),
        resource,
        api_resource,
        job_id.as_str(),
        deployment_id.as_str(),
        environment,
        "Delete",
        "DESTROY",
    )
    .await?;

    // Remove the finalizer to allow deletion
    let finalizers: Vec<String> = resource
        .finalizers()
        .iter()
        .filter(|f| *f != FINALIZER_NAME)
        .cloned()
        .collect();
    let patch_params = PatchParams::default();
    let patch = json!({
        "metadata": {
            "finalizers": finalizers
        }
    });
    let namespaced_api =
        Api::<DynamicObject>::namespaced_with(client.clone(), &namespace, api_resource);
    namespaced_api
        .patch(
            &resource.metadata.name.clone().unwrap(),
            &patch_params,
            &Patch::Merge(&patch),
        )
        .await?;
    println!(
        "Removed finalizer from {}",
        &resource.metadata.name.as_ref().unwrap()
    );

    Ok(())
}

fn is_fatal_error(error: &anyhow::Error) -> bool {
    if let Some(kube_error) = error.downcast_ref::<kube::Error>() {
        return match kube_error {
            kube::Error::Api(api_err) if api_err.code == 401 => true,
            kube::Error::Api(api_err) if api_err.code == 403 => true,
            kube::Error::Api(api_err) if api_err.code == 404 && api_err.reason == "NotFound" => {
                true
            }
            _ => false,
        };
    }

    false
}

pub async fn list_and_apply_modules(
    handler: &GenericCloudHandler,
    client: KubeClient,
) -> Result<(), Box<dyn std::error::Error>> {
    let available_modules = handler.get_all_latest_module("").await.unwrap();
    let available_stack_modules = handler.get_all_latest_stack("").await.unwrap();

    let all_available_modules = [
        available_modules.as_slice(),
        available_stack_modules.as_slice(),
    ]
    .concat();

    for module in all_available_modules {
        let crd_name = format!("{}s.infraweave.io", module.module);

        if crd_already_exists(&client, &crd_name).await {
            println!("CRD {} already exists, skipping", crd_name);
            continue;
        }

        println!("Applying CRD for module: {}", module.module);
        if let Err(e) = apply_module_crd(client.clone(), &module.manifest).await {
            eprintln!("Failed to apply CRD for module {}: {}", module.module, e);
            continue;
        }

        wait_for_crd_to_be_ready(client.clone(), &module.module).await;

        if let Err(e) = fetch_and_apply_exising_deployments(handler, &client, &module).await {
            eprintln!(
                "Failed to fetch existing deployments for module {}: {}",
                module.module, e
            );
        }
    }

    Ok(())
}

async fn crd_already_exists(client: &KubeClient, crd_name: &str) -> bool {
    let crds: Api<CustomResourceDefinition> = Api::all(client.clone());
    matches!(crds.get(crd_name).await, Ok(_))
}

pub fn start_infraweave_watcher(handler: &GenericCloudHandler, client: KubeClient) {
    let handler = handler.clone();

    tokio::spawn(async move {
        println!("Starting unified infraweave.io resource watcher");

        let mut restart_count = 0;
        loop {
            restart_count += 1;

            if restart_count > 1 {
                println!(
                    "Restarting infraweave.io watcher (attempt #{})",
                    restart_count
                );
            }

            match watch_all_infraweave_resources_unified(&handler, client.clone()).await {
                Ok(_) => {
                    println!("Infraweave watcher terminated normally");
                    break;
                }
                Err(e) => {
                    if is_fatal_error(&e) {
                        eprintln!("Fatal error in infraweave watcher: {}. Stopping.", e);
                        break;
                    }

                    let backoff_seconds = std::cmp::min(2u64.pow(restart_count.min(5)), 60);

                    eprintln!(
                        "Infraweave watcher failed (attempt #{}): {}. Restarting in {}s...",
                        restart_count, e, backoff_seconds
                    );

                    tokio::time::sleep(std::time::Duration::from_secs(backoff_seconds)).await;
                }
            }
        }

        println!("Infraweave watcher task has stopped");
    });
}

async fn watch_all_infraweave_resources_unified(
    handler: &GenericCloudHandler,
    client: kube::Client,
) -> anyhow::Result<()> {
    let crds: Api<CustomResourceDefinition> = Api::all(client.clone());
    let crd_list = crds.list(&Default::default()).await?;

    let infraweave_crds: Vec<_> = crd_list
        .items
        .into_iter()
        .filter(|crd| crd.spec.group == KUBERNETES_GROUP)
        .collect();

    if infraweave_crds.is_empty() {
        println!("No infraweave.io CRDs found, waiting...");
        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
        return Ok(());
    }

    println!("Watching {} infraweave.io CRD types", infraweave_crds.len());

    // For DR-reasons, it must be possible to reuse the same cluster-id for multiple clusters,
    // however it need uniqueness to not collide, hence cluster-name is not used here
    let cluster_id = env::var("INFRAWEAVE_CLUSTER_ID").unwrap_or_else(|_| "cluster-id".to_string());

    let (tx, mut rx) = tokio::sync::mpsc::channel(10);

    for crd in infraweave_crds {
        let kind = crd.spec.names.kind.clone();
        let api_resource = get_api_resource(&kind);
        let api = Api::<DynamicObject>::all_with(client.clone(), &api_resource);
        let handler_clone = handler.clone();
        let client_clone = client.clone();
        let cluster_id_clone = cluster_id.clone();
        let tx_clone = tx.clone();

        tokio::spawn(async move {
            let mut watcher_stream = watcher(api, watcher::Config::default()).boxed();

            while let Some(event_result) = watcher_stream.next().await {
                match event_result {
                    Ok(event) => {
                        let result = match event {
                            watcher::Event::Apply(resource) => {
                                handle_resource_apply(
                                    &handler_clone,
                                    &client_clone,
                                    &kind,
                                    resource,
                                    &cluster_id_clone,
                                )
                                .await
                            }
                            watcher::Event::Delete(_) => Ok(()),
                            watcher::Event::Init => {
                                println!("Watcher Init event for {}", kind);
                                Ok(())
                            }
                            watcher::Event::InitDone => {
                                println!("Watcher InitDone event for {}", kind);
                                Ok(())
                            }
                            watcher::Event::InitApply(resource) => {
                                println!(
                                    "Acknowledging existence of {} resource: {:?}",
                                    kind, resource.metadata.name
                                );
                                Ok(())
                            }
                        };

                        if let Err(e) = result {
                            eprintln!("Error handling {} event: {}", kind, e);
                            let _ = tx_clone.send(e).await;
                        }
                    }
                    Err(e) => {
                        eprintln!("Watcher error for {}: {}", kind, e);
                        let _ = tx_clone
                            .send(anyhow::anyhow!("Watcher error for {}: {}", kind, e))
                            .await;
                        break;
                    }
                }
            }

            println!("Watcher for {} has terminated", kind);
        });
    }

    drop(tx);

    while let Some(error) = rx.recv().await {
        if is_fatal_error(&error) {
            return Err(error);
        }
        eprintln!("Received non-fatal error from watcher: {}", error);
    }

    Ok(())
}

async fn fetch_and_apply_exising_deployments(
    handler: &GenericCloudHandler,
    client: &kube::Client,
    module: &ModuleResp,
) -> Result<(), anyhow::Error> {
    let cluster_name = "my-k8s-cluster-1";
    let deployments = match handler
        .get_deployments_using_module(&module.module, cluster_name)
        .await
    {
        Ok(modules) => modules,
        Err(e) => {
            return Err(anyhow::anyhow!(
                "Failed to get deployments using module {}: {:?}",
                module.module,
                e
            ))
        }
    };

    // Group deployments by namespace
    let mut deployments_by_namespace: BTreeMap<String, Vec<_>> = BTreeMap::new();
    for deployment in deployments {
        let namespace = deployment
            .environment
            .split('/')
            .last()
            .unwrap_or("default")
            .to_string();
        deployments_by_namespace
            .entry(namespace)
            .or_insert(vec![])
            .push(deployment);
    }

    for (namespace, deployments) in deployments_by_namespace {
        for deployment in deployments {
            let claim = get_deployment_claim(module, &deployment);
            let dynamic_object: DynamicObject =
                DynamicObject::try_parse(serde_yaml::from_str(&claim).unwrap()).unwrap();
            let api_resource = get_api_resource(&module.module);

            let namespaced_api =
                Api::<DynamicObject>::namespaced_with(client.clone(), &namespace, &api_resource);
            match namespaced_api
                .create(&PostParams::default(), &dynamic_object)
                .await
            {
                Ok(_) => {
                    println!(
                        "Created deployment {} in namespace {}",
                        deployment.deployment_id, namespace
                    );
                }
                Err(e) => {
                    eprintln!(
                        "Failed to create deployment {} in namespace {}: {:?}",
                        deployment.deployment_id, namespace, e
                    );
                }
            }

            let job_id = deployment.job_id;
            let deployment_id = deployment.deployment_id;
            let environment = format!("{}/{}", cluster_name, namespace);

            follow_job_until_finished(
                handler,
                // TODO: optimize?
                client.clone(),
                &dynamic_object,
                &api_resource,
                job_id.as_str(),
                deployment_id.as_str(),
                &environment,
                "Apply",
                "APPLY",
            )
            .await
            .unwrap();
        }
    }

    Ok(())
}

fn get_deployment_claim(module: &ModuleResp, deployment: &DeploymentResp) -> String {
    format!(
        r#"
apiVersion: infraweave.io/v1
kind: {}
metadata:
  name: {}
  namespace: {}
  finalizers:
    - {}
spec:
  moduleVersion: {}
  reference: {}
  variables:
{}
status:
  resourceStatus: {}
"#,
        module.module_name,
        deployment.deployment_id.split('/').last().unwrap(),
        deployment
            .environment
            .split('/')
            .last()
            .unwrap_or("default"),
        FINALIZER_NAME,
        deployment.module_version,
        deployment.reference,
        indent(
            serde_yaml::to_string(&deployment.variables)
                .unwrap()
                .trim_start_matches("---\n"),
            2
        ),
        &deployment.status,
    )
}

async fn wait_for_crd_to_be_ready(client: kube::Client, module: &str) {
    // Wait until the CRD is established
    let crd_name = format!("{}s.infraweave.io", module);
    let crds: Api<CustomResourceDefinition> = Api::all(client.clone());

    // Retry loop to check if CRD is established
    for _attempt in 0..10 {
        match crds.get(&crd_name).await {
            Ok(crd) => {
                if let Some(status) = crd.status {
                    if status
                        .conditions
                        .unwrap_or(vec![])
                        .iter()
                        .any(|cond| cond.type_ == "Established" && cond.status == "True")
                    {
                        println!("CRD {} is established.", crd_name);
                        break;
                    }
                }
            }
            Err(e) => {
                eprintln!("Error getting CRD {}: {:?}", crd_name, e);
            }
        }
        println!(
            "CRD {} not yet established. Retrying... (Attempt {}/10)",
            crd_name, _attempt
        );
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    }
}

async fn update_resource_status(
    client: kube::Client,
    resource: &DynamicObject,
    api_resource: &ApiResource,
    status: &str,
    last_deployment_update: &str,
    message: &str,
    job_id: &str,
) -> Result<(), anyhow::Error> {
    let namespace = resource
        .namespace()
        .unwrap_or_else(|| "default".to_string());
    let namespaced_api = Api::<DynamicObject>::namespaced_with(client, &namespace, api_resource);

    println!(
        "ApiResource details: group='{}', version='{}', kind='{}', plural='{}'",
        api_resource.group, api_resource.version, api_resource.kind, api_resource.plural
    );
    println!(
        "Updating status for resource '{}' in namespace '{}'",
        &resource.metadata.name.clone().unwrap(),
        namespace
    );

    let now = get_timestamp();

    let status_patch = json!({
        "status": {
            "resourceStatus": status,
            "lastDeploymentEvent": last_deployment_update,
            "lastCheck": now,
            "jobId": job_id,
            "lastGeneration": resource.metadata.generation.unwrap_or_default(),
            "logs": message,
        }
    });

    let patch_params = PatchParams::default();

    namespaced_api
        .patch_status(
            &resource.metadata.name.clone().unwrap(),
            &patch_params,
            &Patch::Merge(&status_patch),
        )
        .await?;

    println!(
        "Updated status for {}",
        &resource.metadata.name.clone().unwrap()
    );
    Ok(())
}

// async fn create_secret(client: &kube::Client, namespace: &str) -> Result<(), Box<dyn std::error::Error>> {

//     let secret_data = BTreeMap::from([
//         ("username".to_string(), ByteString(base64::encode("my-username").into_bytes())),
//         ("password".to_string(), ByteString(base64::encode("my-password").into_bytes())),
//     ]);

//     let secret_name = format!("infraweave-secret-test1");

//     let secret = Secret {
//         metadata: kube::api::ObjectMeta {
//             name: Some(secret_name),
//             namespace: Some(namespace.to_string()),
//             ..Default::default()
//         },
//         data: Some(secret_data),
//         ..Default::default()
//     };

//     let secrets: Api<Secret> = Api::namespaced(client.clone(), namespace);
//     let pp = PostParams::default();
//     let result = secrets.create(&pp, &secret).await?;

//     println!("Stored secret {:?} in namespace {}", result.metadata.name, namespace);
//     Ok(())
// }

#[allow(clippy::too_many_arguments)]
async fn follow_job_until_finished(
    handler: &GenericCloudHandler,
    client: kube::Client,
    resource: &DynamicObject,
    api_resource: &ApiResource,
    job_id: &str,
    deployment_id: &str,
    environment: &str,
    event: &str,
    change_type: &str,
) -> Result<(), anyhow::Error> {
    // Polling loop to check job statuses periodically until all are finished
    #[allow(unused_assignments)]
    let mut deployment_status = "".to_string();
    #[allow(unused_assignments)]
    let mut update_time = "".to_string();
    loop {
        let (in_progress, n_job_id, depl_status, depl) =
            is_deployment_in_progress(handler, deployment_id, environment).await;
        deployment_status = depl_status;
        let status = if in_progress {
            "in progress"
        } else {
            "completed"
        };
        let event_status = format!("{} - {}", event, status);

        println!(
            "Checking status of deploymend id {} in environment {} ({} <=> {})",
            deployment_id, environment, job_id, n_job_id
        );

        // Use actual timestamp from deployment if desired and available, otherwise use current time
        update_time = match depl {
            Some(depl) => epoch_to_timestamp(depl.epoch),
            None => "N/A".to_string(),
        };

        let log_str = match handler.read_logs(job_id).await {
            Ok(logs) => {
                let mut log_str = String::new();
                // take the last 10 logs
                for log in logs.iter().rev().take(10).rev() {
                    log_str.push_str(&format!("{}\n", log.message));
                }
                log_str
            }
            Err(e) => e.to_string(),
        };

        match update_resource_status(
            client.clone(),
            resource,
            api_resource,
            &event_status,
            &update_time,
            &log_str,
            job_id,
        )
        .await
        {
            Ok(_) => {
                println!(
                    "Updated status for resource {}",
                    resource.metadata.name.clone().unwrap()
                );
            }
            Err(e) => {
                println!("Failed to update status for resource: {:?}", e);
            }
        };

        if in_progress {
            println!("Status of job {}: {}", job_id, status);
        } else {
            println!("Job is now finished!");
            break;
        }

        std::thread::sleep(std::time::Duration::from_secs(1));
    }

    println!(
        "Fetching change record for deployment {} in environment {}",
        deployment_id, environment
    );

    let change_record = match handler
        .get_change_record(environment, deployment_id, job_id, change_type)
        .await
    {
        Ok(change_record) => {
            println!(
                "Change record for deployment {} in environment {}:\n{}",
                deployment_id, environment, change_record.plan_std_output
            );
            Ok(change_record)
        }
        Err(e) => {
            println!("Failed to get change record: {:?}", e);
            Err(anyhow::anyhow!("Failed to get change record: {:?}", e))
        }
    };

    match update_resource_status(
        client.clone(),
        resource,
        api_resource,
        &format!("{} - {}", event, deployment_status),
        &update_time,
        change_record.unwrap().plan_std_output.as_str(),
        job_id,
    )
    .await
    {
        Ok(_) => {
            println!(
                "Updated status for resource {}",
                resource.metadata.name.clone().unwrap()
            );
        }
        Err(e) => {
            println!("Failed to update status for resource: {:?}", e);
        }
    };

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use env_defs::{DriftDetection, Metadata, ModuleManifest, ModuleSpec};
    use pretty_assertions::assert_eq;

    #[test]
    fn test_get_deployment_claim() {
        let claim = get_deployment_claim(
            &ModuleResp {
                oci_artifact_set: None,
                module: "test-module".to_string(),
                module_name: "TestModule".to_string(),
                manifest: ModuleManifest {
                    metadata: Metadata {
                        name: "test-module".to_string(),
                    },
                    spec: ModuleSpec {
                        module_name: "test-module".to_string(),
                        version: Some("1.0.0".to_string()),
                        description: "Test module".to_string(),
                        reference: "https://test.com".to_string(),
                        examples: None,
                        cpu: None,
                        memory: None,
                    },
                    api_version: "infraweave.io/v1".to_string(),
                    kind: "TestModule".to_string(),
                },
                track: "test-track".to_string(),
                track_version: "beta".to_string(),
                version: "1.0.0-beta".to_string(),
                timestamp: "2021-09-01T00:00:00Z".to_string(),
                module_type: "module".to_string(),
                description: "Test module description".to_string(),
                reference: "https://github.com/project".to_string(),
                tf_variables: vec![],
                tf_outputs: vec![],
                tf_required_providers: vec![],
                tf_lock_providers: vec![],
                tf_extra_environment_variables: vec![],
                s3_key: "test-module-1.0.0-beta".to_string(),
                stack_data: None,
                version_diff: None,
                cpu: "1024".to_string(),
                memory: "2048".to_string(),
            },
            &DeploymentResp {
                epoch: 0,
                deployment_id: "TestModule/test-deployment".to_string(),
                project_id: "12345678910".to_string(),
                region: "us-west-2".to_string(),
                status: "Pending".to_string(),
                job_id: "test-job".to_string(),
                environment: "k8s-cluster-1/test-namespace".to_string(),
                module: "test-module".to_string(),
                module_version: "1.0.0".to_string(),
                module_type: "TestModule".to_string(),
                module_track: "dev".to_string(),
                variables: serde_json::json!({
                    "key1": "key1_value1",
                    "key2": "key2_value2",
                    "complex_map": {
                        "key3": "key3_value3",
                        "key4": ["key4_value1", "key4_value2"]
                    }
                }),
                drift_detection: DriftDetection {
                    enabled: false,
                    interval: "1h".to_string(),
                    auto_remediate: false,
                    webhooks: vec![],
                },
                next_drift_check_epoch: -1,
                has_drifted: false,
                output: serde_json::json!({}),
                policy_results: vec![],
                error_text: "".to_string(),
                deleted: false,
                dependencies: vec![],
                initiated_by: "test-user".to_string(),
                cpu: "1024".to_string(),
                memory: "2048".to_string(),
                reference: "https://github.com/somerepo/somepath/here.yaml".to_string(),
            },
        );
        let expected_claim = r#"
apiVersion: infraweave.io/v1
kind: TestModule
metadata:
  name: test-deployment
  namespace: test-namespace
  finalizers:
    - deletion-handler.finalizer.infraweave.io
spec:
  moduleVersion: 1.0.0
  reference: https://github.com/somerepo/somepath/here.yaml
  variables:
    complex_map:
      key3: key3_value3
      key4:
        - key4_value1
        - key4_value2
    key1: key1_value1
    key2: key2_value2
status:
  resourceStatus: Pending
"#;
        assert_eq!(claim, expected_claim);
    }
}
