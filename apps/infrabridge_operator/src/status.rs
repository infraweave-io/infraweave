use aws_config::meta::region::RegionProviderChain;
use aws_sdk_sqs::types::Message;
use chrono::{DateTime, Utc};
use kube::Client as KubeClient;
use serde_json::Value;
use std::{collections::HashMap, sync::Arc};
use tokio::sync::Mutex;

use log::{debug, error, info, warn};

use tokio::time::Duration;

use crate::finalizer::delete_kind_finalizer;
use crate::other::resume_dependants_apply;
use crate::patch::patch_kind;

use env_aws::read_status;

use aws_sdk_sqs::Client as SqsClient;

pub async fn poll_sqs_messages(
    queue_url: String,
    specs_state: Arc<Mutex<HashMap<String, Value>>>,
) -> Result<(), Box<dyn std::error::Error>> {
    let region_provider = RegionProviderChain::default_provider().or_else("eu-central-1");
    let config = aws_config::from_env().region(region_provider).load().await;
    let sqs_client = SqsClient::new(&config);

    let kube_client = KubeClient::try_default().await?;

    info!("Polling for messages...");
    loop {
        let received_messages = sqs_client
            .receive_message()
            .queue_url(&queue_url)
            .wait_time_seconds(20) // Use long polling
            .send()
            .await?;

        // Correctly handle the Option returned by received_messages.messages()
        for message in received_messages.messages.unwrap_or_default() {
            if let Some(body) = message.body() {
                if let Ok(outer_parsed) = serde_json::from_str::<Value>(body) {
                    // Access the "Message" field and parse it as JSON
                    if let Some(inner_message_str) =
                        outer_parsed.get("Message").and_then(|m| m.as_str())
                    {
                        if let Ok(inner_parsed) = serde_json::from_str::<Value>(inner_message_str) {
                            // Now, extract the deployment_id from the inner JSON
                            if let Some(deployment_id) =
                                inner_parsed.get("deployment_id").and_then(|d| d.as_str())
                            {
                                info!("Deployment ID: {:?}", deployment_id);

                                warn!("Received message: {:?}", inner_parsed);

                                status_check(
                                    deployment_id.to_string(),
                                    specs_state.clone(),
                                    kube_client.clone(),
                                );
                            }
                        }
                    }
                }
            }

            debug!("Acking message: {:?}", message.body());

            if let Some(receipt_handle) = message.receipt_handle() {
                sqs_client
                    .delete_message()
                    .queue_url(&queue_url)
                    .receipt_handle(receipt_handle)
                    .send()
                    .await?;
            }
        }

        tokio::time::sleep(Duration::from_secs(1)).await; // Sleep to prevent constant polling if no messages are available
    }
}

pub async fn subscribe_sqs_log_messages(
    queue_url: String,
    deployment_id: String,
    kind: String,
    name: String,
    plural: String,
    namespace: String,
) -> Result<(), Box<dyn std::error::Error>> {
    let region_provider = RegionProviderChain::default_provider().or_else("eu-central-1");
    let config = aws_config::from_env().region(region_provider).load().await;
    let sqs_client = SqsClient::new(&config);

    let mut inactive_counter = 0;

    warn!("Polling for logs...");
    loop {
        let received_messages = sqs_client
            .receive_message()
            .queue_url(&queue_url)
            .wait_time_seconds(20) // Use long polling
            .send()
            .await?;

        if received_messages.messages.is_none() {
            inactive_counter += 1;
            if inactive_counter > 10 {
                warn!("No messages for 10 rounds, breaking out of loop");
                return Ok(());
            }
        } else {
            inactive_counter = 0;
        }

        // Correctly handle the Option returned by received_messages.messages()
        for message in received_messages.messages.unwrap_or_default() {
            handle_log_message(
                &message,
                &sqs_client,
                &queue_url,
                &deployment_id,
                &kind,
                &name,
                &plural,
                &namespace,
            )
            .await?;
        }

        tokio::time::sleep(Duration::from_secs(1)).await; // Sleep to prevent constant polling if no messages are available
    }
}

async fn handle_log_message(
    message: &Message,
    sqs_client: &SqsClient,
    queue_url: &str,
    deployment_id: &str,
    kind: &str,
    name: &str,
    plural: &str,
    namespace: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(body) = message.body() {
        warn!("Received log: {}", body);

        // Return last 10 lines of the log
        let v = body.split("\n").collect::<Vec<&str>>();
        let messages_string = v.as_slice()[v.len() - std::cmp::min(10, v.len())..]
            .to_vec()
            .join("\n");

        // Store the logs in the specs_state
        patch_kind(
            KubeClient::try_default().await.unwrap(),
            deployment_id.to_string(),
            kind.to_string(),
            name.to_string(),
            plural.to_string(),
            namespace.to_string(),
            serde_json::json!({
                "status": {
                    "logs": messages_string,
                }
            }),
        )
        .await;
    }

    debug!("Acking message: {:?}", message.body());

    if let Some(receipt_handle) = message.receipt_handle() {
        sqs_client
            .delete_message()
            .queue_url(queue_url)
            .receipt_handle(receipt_handle)
            .send()
            .await?;
    }
    Ok(())
}

pub fn status_check(
    deployment_id: String,
    specs_state: Arc<Mutex<HashMap<String, Value>>>,
    kube_client: KubeClient,
) {
    // Schedule a status check
    info!(
        "Fetching status for event with deployment_id {}...",
        deployment_id
    );

    // Spawn a new asynchronous task for the delayed job
    tokio::spawn(async move {
        let status_json = match read_status(deployment_id.clone()).await {
            Ok(status) => status,
            Err(e) => {
                error!("Failed to read status: {:?}", e);
                return;
            }
        };
        warn!("Status fetched for deployment_id: {:?}", status_json);
        // Read json from status
        info!(
            "Will patch status for deployment_id: {} with {:?}",
            status_json.deployment_id, status_json
        );

        // Get the current time in UTC
        let now: DateTime<Utc> = Utc::now();
        // Format the timestamp to RFC 3339 without microseconds
        let timestamp = now.format("%Y-%m-%dT%H:%M:%S%:z").to_string();
        let info = format!("{}: {}", status_json.event, status_json.status);
        // If status_json.status is any of "received", "initiated", set in-progress to true
        let in_progress: &str =
            if status_json.status == "received" || status_json.status == "initiated" {
                "true"
            } else {
                "false"
            };

        // Format is like "infrabridge-worker-eu-central-1-dev:7145bf05-ad57-4f96-a2c3-9ed4220e0c2d"
        let job_id = status_json
            .job_id
            .clone()
            .split(":")
            .collect::<Vec<&str>>()
            .pop()
            .unwrap_or("")
            .to_string();

        let kind = status_json.module.clone();
        let name = status_json.name.clone();
        let plural = status_json.module.clone().to_lowercase() + "s";
        let namespace = "default".to_string();

        if status_json.status == "initiated" {
            // Side effect of only starting on initiated is that logs only update if controller sees this event
            // TODO: ensure only one log puller is running for a deployment_id, now it's possible to start two
            let url = format!(
                "https://sqs.eu-central-1.amazonaws.com/053475148537/logs-{}",
                deployment_id.clone()
            );
            let deployment_id_clone = deployment_id.clone();
            let status = status_json.status.clone();

            let kind = kind.clone();
            let name = name.clone();
            let plural = plural.clone();
            let namespace = namespace.clone();

            tokio::spawn(async move {
                warn!(
                    "Starting log puller for deployment_id: {} during {}",
                    deployment_id_clone.clone(),
                    status.clone()
                );
                let _ = subscribe_sqs_log_messages(
                    url,
                    deployment_id_clone.clone(),
                    kind.clone(),
                    name.clone(),
                    plural.clone(),
                    namespace.clone(),
                )
                .await;
                warn!(
                    "Closing log puller for deployment_id: {} during {}",
                    deployment_id_clone,
                    status.clone()
                );
            });
        }

        patch_kind(
            kube_client.clone(),
            deployment_id.clone(),
            kind,
            name,
            plural,
            namespace,
            serde_json::json!({
                "metadata": {
                    "annotations": {
                        "in-progress": in_progress,
                        "job-id": job_id,
                    }
                },
                "status": {
                    "resourceStatus": info,
                    "lastStatusUpdate": timestamp,
                }
            }),
        )
        .await;

        if status_json.event == "destroy" && status_json.status == "finished" {
            delete_kind_finalizer(
                kube_client,
                status_json.module.clone(),
                status_json.name,
                status_json.module.to_lowercase() + "s",
                "default".to_string(),
                specs_state,
                deployment_id.clone(),
            )
            .await;
        } else if status_json.event == "apply" && status_json.status == "finished" {
            // This is now a finished apply, so check if other resources have dependencies on this
            // and if so, start the apply for those
            resume_dependants_apply(
                kube_client,
                status_json.module.clone(),
                status_json.name,
                "default".to_string(),
            )
            .await;
        }
    });
}
