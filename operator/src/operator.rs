use anyhow;
use env_common::interface::GenericCloudHandler;
use env_common::logic::{is_deployment_in_progress, run_claim};
use env_defs::{CloudProvider, CloudProviderCommon, DeploymentResp, ExtraData, ModuleResp};
use env_utils::{epoch_to_timestamp, get_timestamp, indent};
use k8s_openapi::apiextensions_apiserver::pkg::apis::apiextensions::v1::CustomResourceDefinition;
use kube::api::{ApiResource, DynamicObject, PostParams};
use kube::runtime::controller::{Action, Controller};
use kube::runtime::watcher::Config as WatcherConfig;
use kube::{api::Api, Client as KubeClient, ResourceExt};
use kube_leader_election::{LeaseLock, LeaseLockParams};
use std::collections::BTreeMap;
use std::env;
use std::sync::Arc;
use std::time::Duration;
use tokio::time;

use futures::stream::StreamExt;

use crate::apply::apply_module_crd;
use crate::defs::{FINALIZER_NAME, KUBERNETES_GROUP, NAMESPACE, OPERATOR_NAME};

use kube::api::{Patch, PatchParams};
use serde_json::json;

/// Context passed to reconcile function containing handler and cluster info
#[derive(Clone)]
struct Context {
    handler: GenericCloudHandler,
    client: KubeClient,
    cluster_id: String,
}

pub async fn start_operator(handler: &GenericCloudHandler) -> anyhow::Result<()> {
    let client: KubeClient = initialize_kube_client().await?;
    let leadership = create_lease_lock(client.clone());

    let mut controllers_started = false;

    loop {
        if acquire_leadership_and_run_once(handler, &leadership, &client, &mut controllers_started)
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
    controllers_started: &mut bool,
) -> bool {
    let lease = leadership.try_acquire_or_renew().await.unwrap();

    if lease.acquired_lease {
        println!("Leadership acquired!");
        list_and_apply_modules(handler, client.clone())
            .await
            .unwrap();

        if !*controllers_started {
            start_infraweave_controllers(handler, client.clone());
            *controllers_started = true;
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

/// Starts controllers for all infraweave.io CRDs
/// Uses kube_runtime::Controller for proper reconciliation with requeue
pub fn start_infraweave_controllers(handler: &GenericCloudHandler, client: KubeClient) {
    let handler = handler.clone();
    let client_clone = client.clone();

    tokio::spawn(async move {
        println!("Starting infraweave.io controllers");

        let cluster_id =
            env::var("INFRAWEAVE_CLUSTER_ID").unwrap_or_else(|_| "cluster-id".to_string());

        let ctx = Arc::new(Context {
            handler: handler.clone(),
            client: client_clone.clone(),
            cluster_id: cluster_id.clone(),
        });

        loop {
            match run_controllers(ctx.clone(), client_clone.clone()).await {
                Ok(_) => {
                    println!("Controllers terminated normally");
                    break;
                }
                Err(e) => {
                    // Check for fatal kube errors
                    let is_fatal = if let Some(kube_err) = e.downcast_ref::<kube::Error>() {
                        matches!(
                            kube_err,
                            kube::Error::Api(api_err) if api_err.code == 401 || api_err.code == 403 ||
                                (api_err.code == 404 && api_err.reason == "NotFound")
                        )
                    } else {
                        false
                    };

                    if is_fatal {
                        eprintln!("Fatal error in controllers: {}. Stopping.", e);
                        break;
                    }
                    eprintln!("Controllers failed: {}. Restarting in 10s...", e);
                    tokio::time::sleep(Duration::from_secs(10)).await;
                }
            }
        }

        println!("Controller task has stopped");
    });
}

/// Runs controllers for all infraweave.io CRDs
/// Each CRD gets its own Controller instance that manages reconciliation
async fn run_controllers(ctx: Arc<Context>, client: kube::Client) -> anyhow::Result<()> {
    let crds: Api<CustomResourceDefinition> = Api::all(client.clone());
    let crd_list = crds.list(&Default::default()).await?;

    let infraweave_crds: Vec<_> = crd_list
        .items
        .into_iter()
        .filter(|crd| crd.spec.group == KUBERNETES_GROUP)
        .collect();

    if infraweave_crds.is_empty() {
        println!("No infraweave.io CRDs found, waiting...");
        tokio::time::sleep(Duration::from_secs(5)).await;
        return Ok(());
    }

    println!(
        "Starting controllers for {} infraweave.io CRD types",
        infraweave_crds.len()
    );

    let mut controller_futures = vec![];

    for crd in infraweave_crds {
        let kind = crd.spec.names.kind.clone();
        let api_resource = get_api_resource(&kind);
        let api = Api::<DynamicObject>::all_with(client.clone(), &api_resource);
        let ctx_clone = ctx.clone();
        let kind_clone = kind.clone();

        // Use Controller::new_with which allows specifying the DynamicType (ApiResource)
        // This bypasses the DynamicType: Default requirement
        let controller_future = Controller::new_with(api, WatcherConfig::default(), api_resource)
            .run(
                move |obj, ctx| reconcile(obj, ctx, kind_clone.clone()),
                error_policy,
                ctx_clone,
            )
            .for_each(|res| async move {
                match res {
                    Ok(_) => {}
                    Err(e) => eprintln!("Controller error: {:?}", e),
                }
            });

        controller_futures.push(controller_future);
    }

    // Run all controllers concurrently
    futures::future::join_all(controller_futures).await;

    Ok(())
}

/// Main reconcile function - called by Controller for each resource change
/// This is stateless and crash-safe - all state is read from Kubernetes
async fn reconcile(
    resource: Arc<DynamicObject>,
    ctx: Arc<Context>,
    kind: String,
) -> Result<Action, kube::Error> {
    let name = resource
        .metadata
        .name
        .as_ref()
        .ok_or_else(|| to_kube_err(anyhow::anyhow!("Resource has no name")))?;

    println!("Reconciling {} resource: {}", kind, name);

    let namespace = resource
        .namespace()
        .unwrap_or_else(|| "default".to_string());
    let environment = format!("k8s-{}/{}", ctx.cluster_id, namespace);
    let api_resource = get_api_resource(&kind);

    // Handle deletion
    if resource.metadata.deletion_timestamp.is_some() {
        if resource.finalizers().contains(&FINALIZER_NAME.to_string()) {
            return handle_resource_deletion_nonblocking(
                &ctx.handler,
                &ctx.client,
                &resource,
                &api_resource,
                &kind,
                &environment,
            )
            .await
            .map_err(to_kube_err);
        }
        return Ok(Action::await_change());
    }

    // Add finalizer if missing
    if !resource.finalizers().contains(&FINALIZER_NAME.to_string()) {
        add_finalizer(&ctx.client, &resource, &api_resource, &namespace)
            .await
            .map_err(to_kube_err)?;
        return Ok(Action::requeue(Duration::from_secs(1)));
    }

    // Reconcile the resource (non-blocking)
    // The reconcile function will fetch fresh resource state and determine if work is needed
    reconcile_resource_nonblocking(
        &ctx.handler,
        &ctx.client,
        &resource,
        &api_resource,
        &kind,
        &environment,
    )
    .await
    .map_err(to_kube_err)
}

/// Error policy for the controller - determines requeue behavior on errors
fn error_policy(_resource: Arc<DynamicObject>, error: &kube::Error, _ctx: Arc<Context>) -> Action {
    // Check if it's a fatal kube error (like auth or not found)
    let is_fatal = matches!(
        error,
        kube::Error::Api(api_err) if api_err.code == 401 || api_err.code == 403 ||
            (api_err.code == 404 && api_err.reason == "NotFound")
    );

    if is_fatal {
        eprintln!("Fatal error, not requeuing: {}", error);
        Action::await_change()
    } else {
        eprintln!("Transient error, requeuing in 30s: {}", error);
        Action::requeue(Duration::from_secs(30))
    }
}

/// Non-blocking reconcile - submits job and checks status once, then requeues if needed
async fn reconcile_resource_nonblocking(
    handler: &GenericCloudHandler,
    client: &kube::Client,
    resource: &DynamicObject,
    api_resource: &ApiResource,
    kind: &str,
    environment: &str,
) -> Result<Action, anyhow::Error> {
    let name = resource.metadata.name.as_ref().unwrap();
    let namespace = resource
        .namespace()
        .unwrap_or_else(|| "default".to_string());

    // Re-fetch the resource to get the latest status (Controller cache might be stale)
    let namespaced_api =
        Api::<DynamicObject>::namespaced_with(client.clone(), &namespace, api_resource);
    let fresh_resource = namespaced_api.get(name).await?;

    // Read current status from the fresh resource
    let current_job_id = fresh_resource
        .data
        .get("status")
        .and_then(|s| s.get("jobId"))
        .and_then(|j| j.as_str())
        .unwrap_or("");

    let current_deployment_id = fresh_resource
        .data
        .get("status")
        .and_then(|s| s.get("deploymentId"))
        .and_then(|d| d.as_str())
        .unwrap_or("");

    let current_status = fresh_resource
        .data
        .get("status")
        .and_then(|s| s.get("resourceStatus"))
        .and_then(|r| r.as_str())
        .unwrap_or("");

    println!(
        "Fresh resource status - jobId: '{}', deploymentId: '{}', resourceStatus: '{}'",
        current_job_id, current_deployment_id, current_status
    );

    // Check if reconciliation is needed (generation check)
    // Skip if no active job AND generation hasn't changed AND not in a pending/initiated state
    if current_job_id.is_empty() {
        let observed_generation = fresh_resource
            .data
            .get("status")
            .and_then(|s| s.get("lastGeneration"))
            .and_then(|g| g.as_i64())
            .unwrap_or(0);
        let metadata_generation = fresh_resource.metadata.generation.unwrap_or(0);

        // Check if we're in an "initiated" or "in progress" state without a jobId
        // This shouldn't normally happen - it means the jobId field wasn't in the CRD schema
        // or there's an inconsistency. Give it a few seconds to resolve, then clear the status.
        let is_stuck_state = (current_status.contains("initiated")
            || current_status.contains("in progress"))
            && current_job_id.is_empty();

        if is_stuck_state {
            // Check how long we've been in this inconsistent state
            let last_check = fresh_resource
                .data
                .get("status")
                .and_then(|s| s.get("lastCheck"))
                .and_then(|c| c.as_str())
                .unwrap_or("");

            if !last_check.is_empty() {
                if let Ok(last_check_time) = chrono::DateTime::parse_from_rfc3339(last_check) {
                    let now = chrono::Utc::now();
                    let duration =
                        now.signed_duration_since(last_check_time.with_timezone(&chrono::Utc));

                    if duration.num_seconds() > 30 {
                        println!(
                            "Status '{}' without jobId for {} seconds (likely old CRD schema). Clearing to start fresh.",
                            current_status, duration.num_seconds()
                        );
                        // Clear the inconsistent status
                        let namespace = fresh_resource
                            .namespace()
                            .unwrap_or_else(|| "default".to_string());
                        let namespaced_api = Api::<DynamicObject>::namespaced_with(
                            client.clone(),
                            &namespace,
                            api_resource,
                        );
                        let status_patch = json!({
                            "status": {
                                "resourceStatus": "Ready for reconciliation",
                                "jobId": "",
                            }
                        });
                        let patch_params = PatchParams::default();
                        namespaced_api
                            .patch_status(name, &patch_params, &Patch::Merge(&status_patch))
                            .await?;
                        return Ok(Action::requeue(Duration::from_secs(5)));
                    }
                }
            }

            println!(
                "Status '{}' without jobId - will wait a bit longer for consistency",
                current_status
            );
            return Ok(Action::requeue(Duration::from_secs(5)));
        }

        if observed_generation == metadata_generation && observed_generation > 0 {
            println!(
                "No active job, generation unchanged ({}), skipping reconciliation for {}",
                metadata_generation, name
            );
            return Ok(Action::await_change());
        }

        println!(
            "Will start job: generation changed (observed: {}, current: {}) or first reconcile",
            observed_generation, metadata_generation
        );
    }

    // If no job is running, start one
    if current_job_id.is_empty() {
        println!("Starting new apply job for {} {}", kind, name);

        let yaml = serde_yaml::to_value(&fresh_resource)?;
        let flags = vec![];
        let reference_fallback = "";

        println!(
            "[API-REQUEST] run_claim(apply) - deployment_id: {}/{}, namespace: {}, environment: {}",
            kind.to_lowercase(),
            name,
            namespace,
            environment
        );

        match run_claim(
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
                println!(
                    "[API-RESPONSE] run_claim(apply) - deployment_id: {}, job_id: {}, namespace: {}",
                    deployment_id, job_id, namespace
                );
                println!("Started job {} for deployment {}", job_id, deployment_id);

                // Update status with job info
                update_resource_status(
                    client.clone(),
                    &fresh_resource,
                    api_resource,
                    "Apply - initiated",
                    get_timestamp().as_str(),
                    "Job submitted successfully",
                    &job_id,
                )
                .await?;

                println!(
                    "Status updated with jobId: {}, will check status on next reconcile",
                    job_id
                );

                // Requeue to check job status - give it 10 seconds to start processing
                return Ok(Action::requeue(Duration::from_secs(10)));
            }
            Err(e) => {
                eprintln!("Failed to start job for {}: {}", name, e);
                update_resource_status(
                    client.clone(),
                    &fresh_resource,
                    api_resource,
                    "Apply - failed",
                    get_timestamp().as_str(),
                    &format!("Failed to start job: {}", e),
                    "",
                )
                .await?;
                return Err(e);
            }
        }
    }

    // Job is running, check its status
    println!(
        "Checking status of job {} for deployment {}",
        current_job_id, current_deployment_id
    );

    // Determine if we should do a full deployment check or just fetch logs
    // Full check every 30s, logs every 10s
    let last_check = fresh_resource
        .data
        .get("status")
        .and_then(|s| s.get("lastCheck"))
        .and_then(|c| c.as_str())
        .unwrap_or("");

    let mut should_do_full_check = true;
    let mut time_since_last_check = 0i64;

    if !last_check.is_empty() {
        if let Ok(last_check_time) = chrono::DateTime::parse_from_rfc3339(last_check) {
            let now = chrono::Utc::now();
            let duration = now.signed_duration_since(last_check_time.with_timezone(&chrono::Utc));
            time_since_last_check = duration.num_seconds();

            // If we checked status less than 25 seconds ago, just fetch logs
            // (Use 25s instead of 30s to account for timing variations)
            if time_since_last_check < 25 {
                should_do_full_check = false;
            }
        }
    }

    // If only fetching logs (not full check), just update logs and requeue
    if !should_do_full_check {
        println!(
            "[API-REQUEST] read_logs - deployment_id: {}, job_id: {}, namespace: {} (logs-only check)",
            current_deployment_id, current_job_id, namespace
        );
        println!(
            "Fetching logs for job {} (last full check was {}s ago)",
            current_job_id, time_since_last_check
        );

        let log_str = fetch_job_logs(handler, current_job_id).await;

        // Only update logs, don't change other status fields
        let namespace = resource
            .namespace()
            .unwrap_or_else(|| "default".to_string());
        let namespaced_api =
            Api::<DynamicObject>::namespaced_with(client.clone(), &namespace, api_resource);
        let status_patch = json!({
            "status": {
                "logs": log_str,
            }
        });
        let patch_params = PatchParams::default();
        namespaced_api
            .patch_status(name, &patch_params, &Patch::Merge(&status_patch))
            .await?;

        println!("Updated logs, will fetch again in 10s");
        return Ok(Action::requeue(Duration::from_secs(10)));
    }

    // Do full deployment status check
    println!(
        "[API-REQUEST] is_deployment_in_progress(apply) - deployment_id: {}, namespace: {}, environment: {}",
        current_deployment_id, namespace, environment
    );
    println!("Doing full status check for job {}", current_job_id);
    let (in_progress, _, depl_status, depl) =
        is_deployment_in_progress(handler, current_deployment_id, environment, false, true).await;

    println!(
        "[API-RESPONSE] is_deployment_in_progress(apply) - deployment_id: {}, in_progress: {}, status: {:?}",
        current_deployment_id, in_progress, depl_status
    );

    // If deployment not found and status is still "initiated", the job might not have started yet
    // Give it more time before treating it as failed
    let current_status = fresh_resource
        .data
        .get("status")
        .and_then(|s| s.get("resourceStatus"))
        .and_then(|r| r.as_str())
        .unwrap_or("");

    if !in_progress && depl.is_none() && current_status.contains("initiated") {
        println!(
            "Job {} not found in system yet, will retry in 5 seconds",
            current_job_id
        );
        update_resource_status(
            client.clone(),
            resource,
            api_resource,
            "Apply - pending",
            get_timestamp().as_str(),
            "Waiting for job to start processing",
            current_job_id,
        )
        .await?;
        return Ok(Action::requeue(Duration::from_secs(5)));
    }

    if in_progress {
        // Job still running, update status and requeue
        let status_text = format!("Apply - in progress");
        let update_time = match depl {
            Some(ref d) => epoch_to_timestamp(d.epoch),
            None => get_timestamp(),
        };

        // Don't fetch logs here - logs are fetched every 10s via logs-only checks
        // Just update the status timestamp to reflect we did a full check
        let current_logs = fresh_resource
            .data
            .get("status")
            .and_then(|s| s.get("logs"))
            .and_then(|l| l.as_str())
            .unwrap_or("Checking job status...");

        update_resource_status(
            client.clone(),
            resource,
            api_resource,
            &status_text,
            &update_time,
            current_logs,
            current_job_id,
        )
        .await?;

        // Requeue to check again in 30 seconds (full check)
        println!(
            "Job {} still in progress, will do full check again in 30s",
            current_job_id
        );
        return Ok(Action::requeue(Duration::from_secs(30)));
    }

    // Job completed, get final status
    println!(
        "Job {} completed with status: {}",
        current_job_id, depl_status
    );

    let update_time = match depl {
        Some(ref d) => epoch_to_timestamp(d.epoch),
        None => get_timestamp(),
    };

    // Fetch change record for final status
    println!(
        "[API-REQUEST] get_change_record(apply) - deployment_id: {}, job_id: {}, namespace: {}",
        current_deployment_id, current_job_id, namespace
    );
    let change_record = handler
        .get_change_record(environment, current_deployment_id, current_job_id, "APPLY")
        .await
        .ok();

    println!(
        "[API-RESPONSE] get_change_record(apply) - deployment_id: {}, job_id: {}, found: {}",
        current_deployment_id,
        current_job_id,
        change_record.is_some()
    );

    // Build final message - include error_text if available and change record output
    let mut final_message = String::new();

    // Add error text from deployment if available
    if let Some(ref d) = depl {
        if !d.error_text.is_empty() {
            final_message.push_str("ERROR: ");
            final_message.push_str(&d.error_text);
            final_message.push_str("\n\n");
        }
    }

    // Add change record output if available
    if let Some(ref cr) = change_record {
        final_message.push_str(&cr.plan_std_output);
    } else if final_message.is_empty() {
        final_message.push_str("Job completed");
    }

    // Check if job failed or errored - treat "error" same as "failed" for retry logic
    let is_failure = depl_status == "failed" || depl_status == "error";

    if is_failure {
        let retry_count = fresh_resource
            .data
            .get("status")
            .and_then(|s| s.get("retryCount"))
            .and_then(|r| r.as_i64())
            .unwrap_or(0);

        const MAX_RETRIES: i64 = 3;

        if retry_count < MAX_RETRIES {
            // Update status with failure info but keep jobId for now
            update_resource_status(
                client.clone(),
                resource,
                api_resource,
                &format!("Apply - {}", depl_status),
                &update_time,
                &final_message,
                current_job_id,
            )
            .await?;

            // Increment retry count and clear jobId to trigger new attempt
            let new_retry_count = retry_count + 1;
            update_retry_count(client.clone(), resource, api_resource, new_retry_count).await?;

            // Exponential backoff: 10 minutes * 2^retryCount
            // Retry 1: 10 minutes (600s)
            // Retry 2: 20 minutes (1200s)
            // Retry 3: 40 minutes (2400s)
            let backoff_minutes = 10 * 2_u64.pow(retry_count as u32);
            let backoff = Duration::from_secs(backoff_minutes * 60);

            println!(
                "Job {} (attempt {}/{}). Retrying in {} minutes...",
                depl_status, new_retry_count, MAX_RETRIES, backoff_minutes
            );

            return Ok(Action::requeue(backoff));
        } else {
            // All retries exhausted
            // Check if we need to wait or if 24 hours have passed
            let last_failure_epoch = resource
                .data
                .get("status")
                .and_then(|s| s.get("lastFailureEpoch"))
                .and_then(|e| e.as_u64())
                .unwrap_or(0);

            let now_epoch = env_utils::get_epoch() as u64;
            let twenty_four_hours_ms = 24 * 60 * 60 * 1000;

            if last_failure_epoch == 0 {
                // First time hitting max retries, record the timestamp and clear jobId
                let status_patch = json!({
                    "status": {
                        "lastFailureEpoch": now_epoch,
                        "resourceStatus": format!("Apply - {} (max retries exhausted)", depl_status),
                        "jobId": "",  // Clear jobId
                        "logs": final_message,
                    }
                });

                let namespace = resource
                    .namespace()
                    .unwrap_or_else(|| "default".to_string());
                let namespaced_api =
                    Api::<DynamicObject>::namespaced_with(client.clone(), &namespace, api_resource);
                let patch_params = PatchParams::default();
                namespaced_api
                    .patch_status(
                        &resource.metadata.name.clone().unwrap(),
                        &patch_params,
                        &Patch::Merge(&status_patch),
                    )
                    .await?;

                println!(
                    "Max retries ({}) exhausted. Waiting 24 hours before resetting...",
                    MAX_RETRIES
                );
                return Ok(Action::requeue(Duration::from_secs(24 * 60 * 60)));
            } else if now_epoch - last_failure_epoch >= twenty_four_hours_ms {
                // 24 hours have passed, reset and try again
                reset_retry_count(client.clone(), resource, api_resource).await?;
                println!("24 hours elapsed. Retry count reset. Starting fresh attempt...");
                return Ok(Action::requeue(Duration::from_secs(10)));
            } else {
                // Still waiting for 24 hours to pass - clear jobId to prevent continuous checking
                let status_patch = json!({
                    "status": {
                        "jobId": "",
                        "resourceStatus": format!("Apply - {} (cooling down)", depl_status),
                    }
                });

                let namespace = resource
                    .namespace()
                    .unwrap_or_else(|| "default".to_string());
                let namespaced_api =
                    Api::<DynamicObject>::namespaced_with(client.clone(), &namespace, api_resource);
                let patch_params = PatchParams::default();
                namespaced_api
                    .patch_status(
                        &resource.metadata.name.clone().unwrap(),
                        &patch_params,
                        &Patch::Merge(&status_patch),
                    )
                    .await?;

                let remaining_ms = twenty_four_hours_ms - (now_epoch - last_failure_epoch);
                let remaining_secs = (remaining_ms / 1000) as u64;
                println!(
                    "Still waiting for 24-hour cooling period. {} hours remaining.",
                    remaining_secs / 3600
                );
                return Ok(Action::requeue(Duration::from_secs(remaining_secs.max(60))));
            }
        }
    }

    // Job succeeded - update status and clear jobId, reset retry count
    if depl_status == "successful" {
        let retry_count = fresh_resource
            .data
            .get("status")
            .and_then(|s| s.get("retryCount"))
            .and_then(|r| r.as_i64())
            .unwrap_or(0);

        // Update status with success and clear jobId
        update_resource_status(
            client.clone(),
            resource,
            api_resource,
            &format!("Apply - {}", depl_status),
            &update_time,
            &final_message,
            "", // Clear jobId - job is complete
        )
        .await?;

        if retry_count > 0 {
            reset_retry_count(client.clone(), resource, api_resource).await?;
            println!("Job succeeded, retry count reset");
        }

        println!("Job completed successfully, jobId cleared");
    } else {
        // Unknown status - clear jobId and update status to prevent spam
        update_resource_status(
            client.clone(),
            resource,
            api_resource,
            &format!("Apply - {}", depl_status),
            &update_time,
            &final_message,
            "", // Clear jobId
        )
        .await?;
        println!("Job completed with status: {}, jobId cleared", depl_status);
    }

    // Job is done, wait for next change
    Ok(Action::await_change())
}

/// Non-blocking deletion handler
async fn handle_resource_deletion_nonblocking(
    handler: &GenericCloudHandler,
    client: &kube::Client,
    resource: &DynamicObject,
    api_resource: &ApiResource,
    kind: &str,
    environment: &str,
) -> Result<Action, anyhow::Error> {
    let name = resource.metadata.name.as_ref().unwrap();
    let namespace = resource
        .namespace()
        .unwrap_or_else(|| "default".to_string());

    println!("Handling deletion of {} {}", kind, name);

    // Read current status from CRD
    let current_job_id = resource
        .data
        .get("status")
        .and_then(|s| s.get("jobId"))
        .and_then(|j| j.as_str())
        .unwrap_or("");

    let current_deployment_id = resource
        .data
        .get("status")
        .and_then(|s| s.get("deploymentId"))
        .and_then(|d| d.as_str())
        .unwrap_or("");

    // Check if this is a destroy job or regular apply job
    let is_destroy_job = resource
        .data
        .get("status")
        .and_then(|s| s.get("resourceStatus"))
        .and_then(|r| r.as_str())
        .map(|s| s.contains("Delete"))
        .unwrap_or(false);

    // If no destroy job started yet, start one
    if !is_destroy_job || current_job_id.is_empty() {
        println!("Starting destroy job for {} {}", kind, name);

        // Re-fetch to get latest resource state
        let namespaced_api =
            Api::<DynamicObject>::namespaced_with(client.clone(), &namespace, api_resource);
        let fresh_resource = namespaced_api.get(name).await?;

        let yaml = serde_yaml::to_value(&fresh_resource)?;
        let flags = vec![];
        let reference_fallback = "";

        println!(
            "[API-REQUEST] run_claim(destroy) - deployment_id: {}/{}, namespace: {}, environment: {}",
            kind.to_lowercase(), name, namespace, environment
        );
        match run_claim(
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
                println!(
                    "[API-RESPONSE] run_claim(destroy) - deployment_id: {}, job_id: {}, namespace: {}",
                    deployment_id, job_id, namespace
                );
                println!(
                    "Started destroy job {} for deployment {}",
                    job_id, deployment_id
                );

                update_resource_status(
                    client.clone(),
                    &fresh_resource,
                    api_resource,
                    "Delete - initiated",
                    get_timestamp().as_str(),
                    "Destroy job submitted",
                    &job_id,
                )
                .await?;

                // Requeue to check destroy status - give it 10 seconds to start processing
                return Ok(Action::requeue(Duration::from_secs(10)));
            }
            Err(e) => {
                eprintln!("Failed to start destroy job: {}", e);
                return Err(e);
            }
        }
    }

    // Determine if we should do a full deployment check or just fetch logs
    // Full check every 30s, logs every 10s
    let last_check = resource
        .data
        .get("status")
        .and_then(|s| s.get("lastCheck"))
        .and_then(|c| c.as_str())
        .unwrap_or("");

    let mut should_do_full_check = true;
    let mut time_since_last_check = 0i64;

    if !last_check.is_empty() {
        if let Ok(last_check_time) = chrono::DateTime::parse_from_rfc3339(last_check) {
            let now = chrono::Utc::now();
            let duration = now.signed_duration_since(last_check_time.with_timezone(&chrono::Utc));
            time_since_last_check = duration.num_seconds();

            // If we checked status less than 25 seconds ago, just fetch logs
            if time_since_last_check < 25 {
                should_do_full_check = false;
            }
        }
    }

    // If only fetching logs (not full check), just update logs and requeue
    if !should_do_full_check {
        println!(
            "Fetching logs for destroy job {} (last full check was {}s ago)",
            current_job_id, time_since_last_check
        );

        let log_str = fetch_job_logs(handler, current_job_id).await;

        // Only update logs, don't change other status fields
        let namespace = resource
            .namespace()
            .unwrap_or_else(|| "default".to_string());
        let namespaced_api =
            Api::<DynamicObject>::namespaced_with(client.clone(), &namespace, api_resource);
        let status_patch = json!({
            "status": {
                "logs": log_str,
            }
        });
        let patch_params = PatchParams::default();
        namespaced_api
            .patch_status(name, &patch_params, &Patch::Merge(&status_patch))
            .await?;

        println!("Updated destroy logs, will fetch again in 10s");
        return Ok(Action::requeue(Duration::from_secs(10)));
    }

    // Check destroy job status (full check)
    println!(
        "[API-REQUEST] is_deployment_in_progress(destroy) - deployment_id: {}, namespace: {}, environment: {}",
        current_deployment_id, namespace, environment
    );
    println!("Doing full status check for destroy job {}", current_job_id);
    let (in_progress, _, depl_status, depl) =
        is_deployment_in_progress(handler, current_deployment_id, environment, false, true).await;

    println!(
        "[API-RESPONSE] is_deployment_in_progress(destroy) - deployment_id: {}, in_progress: {}, status: {:?}",
        current_deployment_id, in_progress, depl_status
    );

    if in_progress {
        println!("Destroy job {} still in progress", current_job_id);

        let status_text = "Delete - in progress";
        let update_time = match depl {
            Some(ref d) => epoch_to_timestamp(d.epoch),
            None => get_timestamp(),
        };

        // Preserve existing logs from the logs-only check
        let current_logs = resource
            .data
            .get("status")
            .and_then(|s| s.get("logs"))
            .and_then(|l| l.as_str())
            .unwrap_or("Destroying resources");

        update_resource_status(
            client.clone(),
            resource,
            api_resource,
            status_text,
            &update_time,
            current_logs,
            current_job_id,
        )
        .await?;

        // Requeue to check again in 30 seconds (full check)
        println!(
            "Destroy job {} still in progress, will do full check again in 30s",
            current_job_id
        );
        return Ok(Action::requeue(Duration::from_secs(30)));
    }

    // Destroy completed, check status
    println!(
        "Destroy job {} completed with status: {}",
        current_job_id, depl_status
    );

    let update_time = match depl {
        Some(ref d) => epoch_to_timestamp(d.epoch),
        None => get_timestamp(),
    };

    // Implement retry logic for failed/errored destroy operations
    let is_failure = depl_status == "failed" || depl_status == "error";

    // Build error message if there is one
    let mut error_message = String::new();
    if let Some(ref d) = depl {
        if !d.error_text.is_empty() {
            error_message.push_str("ERROR: ");
            error_message.push_str(&d.error_text);
        }
    }
    if error_message.is_empty() {
        error_message = if is_failure {
            "Destroy operation failed".to_string()
        } else {
            "Destroy operation completed".to_string()
        };
    }

    if is_failure {
        let retry_count = resource
            .data
            .get("status")
            .and_then(|s| s.get("retryCount"))
            .and_then(|r| r.as_i64())
            .unwrap_or(0);

        const MAX_RETRIES: i64 = 3;

        if retry_count < MAX_RETRIES {
            // Update status with failure info but keep jobId for now
            update_resource_status(
                client.clone(),
                resource,
                api_resource,
                &format!("Delete - {}", depl_status),
                &update_time,
                &error_message,
                current_job_id,
            )
            .await?;

            // Increment retry count and clear jobId to trigger new destroy attempt
            let new_retry_count = retry_count + 1;
            update_retry_count(client.clone(), resource, api_resource, new_retry_count).await?;

            // Exponential backoff: 10 minutes * 2^retryCount
            let backoff_minutes = 10 * 2_u64.pow(retry_count as u32);
            let backoff = Duration::from_secs(backoff_minutes * 60);

            println!(
                "Destroy job {} (attempt {}/{}). Retrying in {} minutes...",
                depl_status, new_retry_count, MAX_RETRIES, backoff_minutes
            );

            return Ok(Action::requeue(backoff));
        } else {
            // All retries exhausted
            // Check if we need to wait or if 24 hours have passed
            let last_failure_epoch = resource
                .data
                .get("status")
                .and_then(|s| s.get("lastFailureEpoch"))
                .and_then(|e| e.as_u64())
                .unwrap_or(0);

            let now_epoch = env_utils::get_epoch() as u64;
            let twenty_four_hours_ms = 24 * 60 * 60 * 1000;

            if last_failure_epoch == 0 {
                // First time hitting max retries, record the timestamp and clear jobId
                let status_patch = json!({
                    "status": {
                        "lastFailureEpoch": now_epoch,
                        "resourceStatus": format!("Delete - {} (max retries exhausted)", depl_status),
                        "jobId": "",  // Clear jobId
                    }
                });

                let namespaced_api =
                    Api::<DynamicObject>::namespaced_with(client.clone(), &namespace, api_resource);
                let patch_params = PatchParams::default();
                namespaced_api
                    .patch_status(
                        &resource.metadata.name.clone().unwrap(),
                        &patch_params,
                        &Patch::Merge(&status_patch),
                    )
                    .await?;

                println!(
                    "Max destroy retries ({}) exhausted. Waiting 24 hours before resetting...",
                    MAX_RETRIES
                );
                return Ok(Action::requeue(Duration::from_secs(24 * 60 * 60)));
            } else if now_epoch - last_failure_epoch >= twenty_four_hours_ms {
                // 24 hours have passed, reset and try again
                reset_retry_count(client.clone(), resource, api_resource).await?;
                println!("24 hours elapsed. Destroy retry count reset. Starting fresh attempt...");
                return Ok(Action::requeue(Duration::from_secs(10)));
            } else {
                // Still waiting for 24 hours to pass - clear jobId to prevent continuous checking
                let status_patch = json!({
                    "status": {
                        "jobId": "",
                        "resourceStatus": format!("Delete - {} (cooling down)", depl_status),
                    }
                });

                let namespaced_api =
                    Api::<DynamicObject>::namespaced_with(client.clone(), &namespace, api_resource);
                let patch_params = PatchParams::default();
                namespaced_api
                    .patch_status(
                        &resource.metadata.name.clone().unwrap(),
                        &patch_params,
                        &Patch::Merge(&status_patch),
                    )
                    .await?;

                let remaining_ms = twenty_four_hours_ms - (now_epoch - last_failure_epoch);
                let remaining_secs = (remaining_ms / 1000) as u64;
                println!(
                    "Still waiting for 24-hour cooling period. {} hours remaining.",
                    remaining_secs / 3600
                );
                return Ok(Action::requeue(Duration::from_secs(remaining_secs.max(60))));
            }
        }
    }

    // Destroy succeeded, remove finalizer to allow deletion
    if depl_status == "successful" {
        println!("Destroy successful, removing finalizer from {}", name);

        // Update status with success and clear jobId
        update_resource_status(
            client.clone(),
            resource,
            api_resource,
            &format!("Delete - {}", depl_status),
            &update_time,
            &error_message, // Will be "Destroy operation completed"
            "",             // Clear jobId
        )
        .await?;

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

        println!("Removed finalizer from {}", name);
    }

    Ok(Action::await_change())
}

/// Fetches existing deployments and creates CRDs for them
/// The controller will then reconcile them automatically (non-blocking)
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
                        "Created CRD for deployment {} in namespace {} - controller will reconcile it",
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
        }
    }

    println!("Existing deployments imported as CRDs. Controllers will reconcile them.");
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

/// Helper function to fetch logs with user-friendly error messages
async fn fetch_job_logs(handler: &GenericCloudHandler, job_id: &str) -> String {
    println!("[API-REQUEST] read_logs - job_id: {}", job_id);
    match handler.read_logs(job_id).await {
        Ok(logs) => {
            println!(
                "[API-RESPONSE] read_logs - job_id: {}, log_count: {}",
                job_id,
                logs.len()
            );
            if logs.is_empty() {
                "No logs available yet - job is initializing...".to_string()
            } else {
                let mut log_str = String::new();
                for log in logs.iter().rev().take(10).rev() {
                    log_str.push_str(&format!("{}\n", log.message));
                }
                log_str
            }
        }
        Err(e) => {
            println!(
                "[API-RESPONSE] read_logs - job_id: {}, error: {}",
                job_id, e
            );
            "No logs available yet - job is initializing...".to_string()
        }
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

    // Calculate deployment_id from resource kind and name
    let kind = &api_resource.kind;
    let name = resource.metadata.name.as_ref().unwrap();
    let deployment_id = format!("{}/{}", kind.to_lowercase(), name);

    // Preserve existing retry count
    let retry_count = resource
        .data
        .get("status")
        .and_then(|s| s.get("retryCount"))
        .and_then(|r| r.as_i64())
        .unwrap_or(0);

    let status_patch = json!({
        "status": {
            "resourceStatus": status,
            "lastDeploymentEvent": last_deployment_update,
            "lastCheck": now,
            "jobId": job_id,
            "deploymentId": deployment_id,
            "lastGeneration": resource.metadata.generation.unwrap_or_default(),
            "logs": message,
            "retryCount": retry_count,
        }
    });

    println!(
        "Status patch being applied: {}",
        serde_json::to_string_pretty(&status_patch).unwrap()
    );

    let patch_params = PatchParams::default();

    let result = namespaced_api
        .patch_status(
            &resource.metadata.name.clone().unwrap(),
            &patch_params,
            &Patch::Merge(&status_patch),
        )
        .await?;

    println!(
        "Updated status for {} - result resourceVersion: {:?}",
        &resource.metadata.name.clone().unwrap(),
        result.metadata.resource_version
    );

    // Verify the update by reading back
    let verify = namespaced_api
        .get(&resource.metadata.name.clone().unwrap())
        .await?;
    let verify_job_id = verify
        .data
        .get("status")
        .and_then(|s| s.get("jobId"))
        .and_then(|j| j.as_str())
        .unwrap_or("");
    println!(
        "Verified jobId after update: '{}' (expected: '{}')",
        verify_job_id, job_id
    );

    Ok(())
}

/// Update retry count in resource status
async fn update_retry_count(
    client: kube::Client,
    resource: &DynamicObject,
    api_resource: &ApiResource,
    retry_count: i64,
) -> Result<(), anyhow::Error> {
    let namespace = resource
        .namespace()
        .unwrap_or_else(|| "default".to_string());
    let namespaced_api = Api::<DynamicObject>::namespaced_with(client, &namespace, api_resource);

    let status_patch = json!({
        "status": {
            "retryCount": retry_count,
            "jobId": "",  // Clear jobId to trigger new job
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
        "Updated retry count to {} for {}",
        retry_count,
        resource.metadata.name.as_ref().unwrap()
    );
    Ok(())
}

/// Reset retry count (used after 24-hour cooling period)
async fn reset_retry_count(
    client: kube::Client,
    resource: &DynamicObject,
    api_resource: &ApiResource,
) -> Result<(), anyhow::Error> {
    let namespace = resource
        .namespace()
        .unwrap_or_else(|| "default".to_string());
    let namespaced_api = Api::<DynamicObject>::namespaced_with(client, &namespace, api_resource);

    let status_patch = json!({
        "status": {
            "retryCount": 0,
            "jobId": "",  // Clear jobId to trigger fresh attempt
            "lastFailureEpoch": null,  // Clear failure timestamp
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
        "Reset retry count for {}",
        resource.metadata.name.as_ref().unwrap()
    );
    Ok(())
}

fn to_kube_err(e: anyhow::Error) -> kube::Error {
    kube::Error::Service(Box::new(std::io::Error::new(
        std::io::ErrorKind::Other,
        e.to_string(),
    )))
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
                        providers: Vec::with_capacity(0),
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
                tf_providers: Vec::with_capacity(0),
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
                tf_resources: None,
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
    key1: key1_value1
    key2: key2_value2
    complex_map:
      key3: key3_value3
      key4:
        - key4_value1
        - key4_value2
status:
  resourceStatus: Pending
"#;
        assert_eq!(claim, expected_claim);
    }
}
