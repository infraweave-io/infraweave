use aws_sdk_sqs::types::Message;
use kube::Client as KubeClient;
use serde_json::Value;
use std::{collections::HashMap, sync::Arc};
use tokio::sync::Mutex;

use log::{info, warn};

use crate::patch::patch_kind;

pub struct PassthroughSpecData {
    pub kind: String,
    pub name: String,
    pub plural: String,
    pub namespace: String,
    pub deployment_id: String,
}

// pub type SqsMessageHandler = dyn Fn(
//         Arc<Mutex<HashMap<String, Value>>>,
//         KubeClient,
//         Message,
//         Option<&PassthroughSpecData>,
//     )
//         -> std::pin::Pin<Box<dyn futures::Future<Output = Result<(), Box<anyhow::Error>>> + Send>>
//     + Send
//     + Sync;

// pub async fn poll_sqs_messages(
//     queue_url: String,
//     specs_state: Arc<Mutex<HashMap<String, Value>>>,
//     message_handler: Arc<SqsMessageHandler>,
//     inactive_counter_limit: u32,
//     extra_data: Option<PassthroughSpecData>,
// ) -> Result<(), Box<dyn std::error::Error>> {
//     let region_provider = RegionProviderChain::default_provider().or_else("eu-central-1");
//     let config = aws_config::from_env().region(region_provider).load().await;
//     let sqs_client = SqsClient::new(&config);

//     let kube_client = KubeClient::try_default().await?;

//     let mut inactive_counter = 0;

//     info!("Polling for messages...");
//     loop {
//         let received_messages = sqs_client
//             .receive_message()
//             .queue_url(&queue_url)
//             .wait_time_seconds(20) // Use long polling
//             .send()
//             .await?;

//         if inactive_counter_limit > 0 {
//             // If inactive_counter_limit is set, check for inactivity and stop polling if no messages are received
//             warn!(
//                 "Inactive counter limit is set to {}",
//                 inactive_counter_limit
//             );
//             if received_messages.messages.is_none() {
//                 inactive_counter += 1;
//                 if inactive_counter > inactive_counter_limit {
//                     warn!(
//                         "No messages for {} rounds, breaking out of loop",
//                         inactive_counter_limit
//                     );
//                     return Ok(());
//                 }
//             } else {
//                 inactive_counter = 0;
//             }
//         }

//         // Correctly handle the Option returned by received_messages.messages()
//         for message in received_messages.messages.unwrap_or_default() {
//             match message_handler.clone()(
//                 specs_state.clone(),
//                 kube_client.clone(),
//                 message.clone(),
//                 extra_data.as_ref(),
//             )
//             .await
//             {
//                 Ok(_) => {
//                     debug!("Acking message: {:?}", message.body());
//                     if let Some(receipt_handle) = message.receipt_handle() {
//                         sqs_client
//                             .delete_message()
//                             .queue_url(queue_url.clone())
//                             .receipt_handle(receipt_handle)
//                             .send()
//                             .await?;
//                     }
//                 }
//                 Err(e) => {
//                     error!("Error handling message: {}", e);
//                 }
//             }
//         }

//         tokio::time::sleep(Duration::from_secs(1)).await; // Sleep to prevent constant polling if no messages are available
//     }
// }

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
    // tokio::spawn(async move {
    //     let status_json = match read_status(deployment_id.clone()).await {
    //         Ok(status) => status,
    //         Err(e) => {
    //             error!("Failed to read status: {:?}", e);
    //             return;
    //         }
    //     };
    //     warn!("Status fetched for deployment_id: {:?}", status_json);
    //     // Read json from status
    //     info!(
    //         "Will patch status for deployment_id: {} with {:?}",
    //         status_json.deployment_id, status_json
    //     );

    //     // Get the current time in UTC
    //     let now: DateTime<Utc> = Utc::now();
    //     // Format the timestamp to RFC 3339 without microseconds
    //     let timestamp = now.format("%Y-%m-%dT%H:%M:%S%:z").to_string();
    //     let info = format!("{}: {}", status_json.event, status_json.status);
    //     // If status_json.status is any of "received", "initiated", set in-progress to true
    //     let in_progress: &str =
    //         if status_json.status == "received" || status_json.status == "initiated" {
    //             "true"
    //         } else {
    //             "false"
    //         };

    //     // Format is like "infraweave-worker-eu-central-1-dev:7145bf05-ad57-4f96-a2c3-9ed4220e0c2d"
    //     let job_id = status_json
    //         .job_id
    //         .clone()
    //         .split(":")
    //         .collect::<Vec<&str>>()
    //         .pop()
    //         .unwrap_or("")
    //         .to_string();

    //     let kind = status_json.module.clone();
    //     let name = status_json.name.clone();
    //     let plural = status_json.module.clone().to_lowercase() + "s";
    //     let namespace = "default".to_string();

    //     if status_json.status == "initiated" {
    //         // Side effect of only starting on initiated is that logs only update if controller sees this event
    //         // TODO: ensure only one log puller is running for a deployment_id, now it's possible to start two
    //         let queue_url = format!(
    //             "https://sqs.eu-central-1.amazonaws.com/053475148537/logs-{}",
    //             deployment_id.clone()
    //         );
    //         let deployment_id_clone = deployment_id.clone();
    //         let status = status_json.status.clone();

    //         let kind = kind.clone();
    //         let name = name.clone();
    //         let plural = plural.clone();
    //         let namespace = namespace.clone();
    //         let specs_state = specs_state.clone();

    //         tokio::spawn(async move {
    //             warn!(
    //                 "Starting log puller for deployment_id: {} during {}",
    //                 deployment_id_clone.clone(),
    //                 status.clone()
    //             );

    //             let inactive_counter_limit = 10; // Stop polling after no messages for 10 rounds
    //             let extra_data = PassthroughSpecData {
    //                 // Extra step in order to pass data to the handler through the poll_sqs_messages function
    //                 // since data can't be passed directly to the handler
    //                 kind: kind.clone(),
    //                 name: name.clone(),
    //                 plural: plural.clone(),
    //                 namespace: namespace.clone(),
    //                 deployment_id: deployment_id_clone.clone(),
    //             };
    //             let handler: Arc<SqsMessageHandler> = Arc::new(|state, client, msg, extra| {
    //                 let extra = extra.unwrap();

    //                 Box::pin(on_sqs_log_message(
    //                     state,
    //                     client,
    //                     msg,
    //                     extra.kind.clone(),
    //                     extra.name.clone(),
    //                     extra.plural.clone(),
    //                     extra.namespace.clone(),
    //                     extra.deployment_id.clone(),
    //                 )) // Wrap in Box::pin for Future
    //             });
    //             if let Err(e) = poll_sqs_messages(
    //                 queue_url,
    //                 specs_state,
    //                 handler,
    //                 inactive_counter_limit,
    //                 Some(extra_data),
    //             )
    //             .await
    //             {
    //                 error!("Failed to poll SQS messages: {}", e);
    //             }
    //             warn!(
    //                 "Closing log puller for deployment_id: {} during {}",
    //                 deployment_id_clone,
    //                 status.clone()
    //             );
    //         });
    //     }

    //     patch_kind(
    //         kube_client.clone(),
    //         deployment_id.clone(),
    //         kind,
    //         name,
    //         plural,
    //         namespace,
    //         serde_json::json!({
    //             "metadata": {
    //                 "annotations": {
    //                     "in-progress": in_progress,
    //                     "job-id": job_id,
    //                 }
    //             },
    //             "status": {
    //                 "resourceStatus": info,
    //                 "lastStatusUpdate": timestamp,
    //             }
    //         }),
    //     )
    //     .await;

    //     if status_json.event == "destroy" && status_json.status == "finished" {
    //         delete_kind_finalizer(
    //             kube_client,
    //             status_json.module.clone(),
    //             status_json.name,
    //             status_json.module.to_lowercase() + "s",
    //             "default".to_string(),
    //             specs_state,
    //             deployment_id.clone(),
    //         )
    //         .await;
    //     } else if status_json.event == "apply" && status_json.status == "finished" {
    //         // This is now a finished apply, so check if other resources have dependencies on this
    //         // and if so, start the apply for those
    //         resume_dependants_apply(
    //             kube_client,
    //             status_json.module.clone(),
    //             status_json.name,
    //             "default".to_string(),
    //         )
    //         .await;
    //     }
    // });
}

async fn on_sqs_log_message(
    _specs_state: Arc<Mutex<HashMap<String, serde_json::Value>>>,
    kube_client: kube::Client,
    message: Message,
    kind: String,
    name: String,
    plural: String,
    namespace: String,
    deployment_id: String,
) -> Result<(), Box<anyhow::Error>> {
    if let Some(body) = message.body() {
        warn!("Received log: {}", body);

        // Return last 10 lines of the log
        let v = body.split("\n").collect::<Vec<&str>>();
        let messages_string = v.as_slice()[v.len() - std::cmp::min(10, v.len())..]
            .to_vec()
            .join("\n");

        patch_kind(
            kube_client,
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
    Ok(())
}
