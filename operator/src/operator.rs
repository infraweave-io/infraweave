use env_common::interface::GenericCloudHandler;
use env_common::logic::{is_deployment_in_progress, run_claim};
use env_defs::{CloudProvider, CloudProviderCommon, DeploymentResp, ModuleResp};
use env_utils::{epoch_to_timestamp, get_timestamp, indent};
use futures::TryStreamExt;
use k8s_openapi::apiextensions_apiserver::pkg::apis::apiextensions::v1::CustomResourceDefinition;
use kube::api::{ApiResource, DynamicObject, PostParams};
use kube::{api::Api, runtime::watcher, Client as KubeClient};
use kube_leader_election::{LeaseLock, LeaseLockParams};
use std::collections::{BTreeMap, HashSet};
use std::time::Duration;
use tokio::time;

use futures::stream::StreamExt;
use log::{info, warn};

use crate::apply::apply_module_crd;
use crate::defs::{FINALIZER_NAME, KUBERNETES_GROUP, NAMESPACE, OPERATOR_NAME};

use kube::api::{Patch, PatchParams, ResourceExt};
use serde_json::json;

pub async fn start_operator(
    handler: &GenericCloudHandler,
) -> Result<(), Box<dyn std::error::Error>> {
    let client: KubeClient = initialize_kube_client().await?;
    let leadership = create_lease_lock(client.clone());

    loop {
        if acquire_leadership_and_run_once(handler, &leadership, &client).await {
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
) -> bool {
    match leadership.try_acquire_or_renew().await {
        Ok(lease) => {
            if lease.acquired_lease {
                println!("Acquired leadership as {}", get_holder_id());

                let current_environment =
                    std::env::var("INFRAWEAVE_ENVIRONMENT").unwrap_or_else(|_| "dev".to_string());
                info!("Current environment: {}", current_environment);

                let modules_watched_set: HashSet<String> = HashSet::new();
                match list_and_apply_modules(
                    handler,
                    client.clone(),
                    &current_environment,
                    &modules_watched_set,
                )
                .await
                {
                    Ok(_) => println!("Successfully listed and applied modules"),
                    Err(e) => eprintln!("Failed to list and apply modules: {:?}", e),
                }
                true
            } else {
                println!("Failed to acquire leadership as {}", get_holder_id());
                false
            }
        }
        Err(e) => {
            eprintln!("Error during leadership acquisition: {:?}", e);
            false
        }
    }
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

async fn watch_all_infraweave_resources(
    handler: &GenericCloudHandler,
    client: kube::Client,
    kind: String,
) -> Result<(), Box<dyn std::error::Error>> {
    let api_resource = get_api_resource(&kind);

    let api = Api::<DynamicObject>::all_with(client.clone(), &api_resource);
    let list_params = watcher::Config::default();
    let mut resource_watcher = watcher(api, list_params).boxed();

    let cluster_name = "my-k8s-cluster-1".to_string(); // TODO: Get cluster name from env

    while let Some(event) = resource_watcher.try_next().await? {
        match event {
            watcher::Event::Apply(resource) => {
                let namespace = resource
                    .namespace()
                    .unwrap_or_else(|| "default".to_string());
                let environment = format!("{}/{}", cluster_name, namespace);

                println!("Resource applied: {:?}", resource);
                if resource.metadata.deletion_timestamp.is_none() {
                    println!("Resource is not being deleted");
                    // Resource is not being deleted
                    if !resource.finalizers().contains(&FINALIZER_NAME.to_string()) {
                        // Add the finalizer
                        let patch_params = PatchParams::default();
                        let patch = json!({
                            "metadata": {
                                "finalizers": [FINALIZER_NAME]
                            }
                        });
                        let namespaced_api = Api::<DynamicObject>::namespaced_with(
                            client.clone(),
                            &namespace,
                            &api_resource,
                        );
                        namespaced_api
                            .patch(
                                &resource.metadata.name.clone().unwrap(),
                                &patch_params,
                                &Patch::Merge(&patch),
                            )
                            .await?;
                        println!("Added finalizer to {:?}", resource.metadata.name.unwrap());
                    } else {
                        println!("Resource has finalizer");
                        println!("Checking if resource has different lastGeneration");

                        if let Some(status) = resource.data.get("status") {
                            let observed_generation = status
                                .get("lastGeneration")
                                .and_then(|g| g.as_i64())
                                .unwrap_or(0);
                            let metadata_generation = resource.metadata.generation.unwrap_or(0);

                            if observed_generation == metadata_generation {
                                println!(
                                    "Generation has not changed; skipping reconciliation for {}",
                                    resource.metadata.name.unwrap()
                                );
                                continue;
                            } else {
                                println!(
                                    "Generation has changed from {} to {}; reconciling",
                                    observed_generation, metadata_generation
                                );
                            }
                        }

                        // Process the resource normally
                        let yaml = serde_yaml::to_value(&resource).unwrap();
                        println!("Applying {} manifest \n{:?}", kind, resource);
                        let (job_id, deployment_id) =
                            match run_claim(handler, &yaml, &environment, "apply").await {
                                Ok((job_id, deployment_id)) => {
                                    println!("Successfully applied {} manifest", kind);
                                    Ok((job_id, deployment_id))
                                }
                                Err(e) => Err(anyhow::anyhow!(
                                    "Failed to apply {} manifest: {:?}",
                                    kind,
                                    e
                                )),
                            }
                            .unwrap(); // TODO: Handle error, e.g. invalid variables should indicate a failed resource

                        follow_job_until_finished(
                            handler,
                            client.clone(),
                            &resource,
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
                } else {
                    // Resource is being deleted
                    if resource.finalizers().contains(&FINALIZER_NAME.to_string()) {
                        // Perform cleanup before deletion
                        let yaml = serde_yaml::to_value(&resource).unwrap();
                        println!("Deleting {} manifest \n{:?}", kind, resource);
                        let (job_id, deployment_id) =
                            match run_claim(handler, &yaml, &environment, "destroy").await {
                                Ok((job_id, deployment_id)) => {
                                    println!("Successfully requested destroying {} manifest", kind);
                                    update_resource_status(
                                        client.clone(),
                                        &resource,
                                        &api_resource,
                                        "Deleted requested",
                                        get_timestamp().as_str(),
                                        "Resource deletetion requested successfully",
                                        &job_id,
                                    )
                                    .await?;
                                    Ok((job_id, deployment_id))
                                }
                                Err(e) => Err(anyhow::anyhow!(
                                    "Failed to request destroying {} manifest: {:?}",
                                    kind,
                                    e
                                )),
                            }
                            .unwrap();

                        follow_job_until_finished(
                            handler,
                            client.clone(),
                            &resource,
                            &api_resource,
                            job_id.as_str(),
                            deployment_id.as_str(),
                            &environment,
                            "Delete",
                            "DESTROY",
                        )
                        .await
                        .unwrap();

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
                        let namespaced_api = Api::<DynamicObject>::namespaced_with(
                            client.clone(),
                            &namespace,
                            &api_resource,
                        );
                        namespaced_api
                            .patch(
                                &resource.metadata.name.clone().unwrap(),
                                &patch_params,
                                &Patch::Merge(&patch),
                            )
                            .await?;
                        println!(
                            "Removed finalizer from {}",
                            &resource.metadata.name.unwrap()
                        );
                    }
                }
            }
            watcher::Event::Delete(_) => {
                // Resource has been fully deleted
                // TODO: Perform cleanup here if needed
            }
            watcher::Event::Init => {
                println!("Watcher Init event");
            }
            watcher::Event::InitDone => {
                eprintln!("Watcher InitDone event");
            }
            watcher::Event::InitApply(resource) => {
                println!(
                    "Acknowledging existence of {} resource: {:?}",
                    kind, resource
                );
                // TODO: Maybe call reconcile logic here
            }
        }
    }
    Ok(())
}

async fn initialize_kube_client() -> Result<KubeClient, Box<dyn std::error::Error>> {
    Ok(KubeClient::try_default().await?)
}

pub async fn list_and_apply_modules(
    handler: &GenericCloudHandler,
    client: KubeClient,
    environment: &str,
    modules_watched_set: &HashSet<String>,
) -> Result<(), Box<dyn std::error::Error>>
where {
    let available_modules = handler.get_all_latest_module(environment).await.unwrap();

    let available_stack_modules = handler.get_all_latest_stack(environment).await.unwrap();

    let all_available_modules = [
        available_modules.as_slice(),
        available_stack_modules.as_slice(),
    ]
    .concat();

    for module in all_available_modules {
        if modules_watched_set.contains(&module.module) {
            warn!("Module {} already being watched", module.module);
            continue;
        }
        apply_crd_and_start_watching(handler, client.clone(), &module, modules_watched_set)
            .unwrap();
    }

    Ok(())
}

fn apply_crd_and_start_watching(
    handler: &GenericCloudHandler,
    client: kube::Client,
    module: &ModuleResp,
    modules_watched_set: &HashSet<String>,
) -> Result<(), anyhow::Error> {
    if modules_watched_set.contains(&module.module) {
        warn!("Module {} already being watched", module.module);
        return Ok(());
    }

    let client = client.clone();
    let module = module.clone();
    let handler = handler.clone();
    tokio::spawn(async move {
        match apply_module_crd(client.clone(), &module.manifest).await {
            Ok(_) => {
                println!("Applied CRD for module {}", module.module);
            }
            Err(e) => {
                eprintln!("Failed to apply CRD for module {}: {:?}", module.module, e);
            }
        }

        wait_for_crd_to_be_ready(client.clone(), &module.module).await;

        fetch_and_apply_exising_deployments(&handler, &client, &module)
            .await
            .unwrap();

        match watch_all_infraweave_resources(&handler, client.clone(), module.module.clone()).await
        {
            Ok(_) => {
                println!("Watching resources for module {}", module.module);
                Ok(())
            }
            Err(e) => {
                println!(
                    "Failed to watch resources for module {}: {:?}",
                    module.module, e
                );
                Err(format!(
                    "Failed to watch resources for module {}: {:?}",
                    module.module, e
                ))
            }
        }
    });
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
) -> Result<(), Box<dyn std::error::Error>> {
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
                environment: "cluster-name/test-namespace".to_string(),
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
