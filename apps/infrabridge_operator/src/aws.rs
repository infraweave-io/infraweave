use aws_sdk_lambda::{Client, Error as AwsError};
use aws_sdk_lambda::types::InvocationType;
use aws_sdk_lambda::primitives::Blob;
use aws_sdk_sqs::types::QueueAttributeName;
use chrono::{DateTime, Utc};
use kube::api::{ApiResource, DynamicObject, GroupVersionKind};
use kube::{Api, Client as KubeClient};
use serde::{Serialize, Deserialize};
use log::{debug, error, info, warn};
use serde_json::Value;

#[derive(Debug, Serialize, Deserialize)]
struct ApiInfraLambdaPayload {
    event: String,
    module: String,
    name: String,
    deployment_id: String,
    spec: serde_json::value::Value,
    annotations: serde_json::value::Value,
}

#[derive(Debug, Serialize, Deserialize)]
struct ApiStatusLambdaPayload {
    deployment_id: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ApiStatusResult {
    deployment_id: String,
    status: String,
    epoch: i64,
    event: String,
    module: String,
    name: String,
    // spec: serde_json::value::Value,
    // manifest: serde_json::value::Value,
}

pub async fn mutate_infra(event: String, module: String, name: String, deployment_id: String, spec: serde_json::value::Value, annotations: serde_json::value::Value) -> Result<(), AwsError> {
    
    let payload = ApiInfraLambdaPayload {
        event: event.clone(),
        module: module.clone(),
        name: name.clone(),
        deployment_id: deployment_id.clone(),
        spec: spec,
        annotations: annotations,
    };
    
    let shared_config = aws_config::from_env().load().await; 
    let region_name = shared_config.region().unwrap();

    let client = Client::new(&shared_config);
    let api_function_name = "infrastructureApi";

    let serialized_payload = serde_json::to_vec(&payload).unwrap();
    let payload_blob = Blob::new(serialized_payload);

    warn!("Invoking {}-job {} in region {} using {} with payload: {:?}", event, deployment_id, region_name, api_function_name, payload);

    let request = client.invoke()
        .function_name(api_function_name)
        .invocation_type(InvocationType::RequestResponse)
        .payload(payload_blob);

    let response = match request.send().await {
        Ok(response) => response,
        Err(e) => {
            error!("Failed to invoke Lambda: {}", e);
            return Err(e.into());
        },
    };

    if let Some(blob) = response.payload {
        let bytes = blob.into_inner(); // Gets the Vec<u8>
        let response_string = String::from_utf8(bytes).expect("response not valid UTF-8");
        warn!("Lambda response: {:?}", response_string);
        let parsed_json: Value = serde_json::from_str(&response_string).expect("response not valid JSON");
        warn!("Parsed JSON: {:?}", parsed_json);
        // Although we get the deployment id, the name and namespace etc is unique within the cluster
        // and patching it here causes a race condition, so we should not do it here

        let body = parsed_json.get("body").expect("body not found").as_str().expect("body not a string");
        let body_json: Value = serde_json::from_str(body).expect("body not valid JSON");
        let deployment_id = body_json.get("deployment_id").expect("deployment_id not found").as_str().expect("deployment_id not a string");
        warn!("Deployment ID: {:?}", deployment_id);
        // Get the current time in UTC
        let now: DateTime<Utc> = Utc::now();
        // Format the timestamp to RFC 3339 without microseconds
        let timestamp = now.format("%Y-%m-%dT%H:%M:%S%:z").to_string();
        patch_kind(
            KubeClient::try_default().await.unwrap(),
            deployment_id.to_string(),
            module.clone(),
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
            })
        ).await;
    }

    Ok(())
}


pub async fn read_status(deployment_id: String) -> Result<ApiStatusResult, Box<dyn std::error::Error>> {
    let payload = ApiStatusLambdaPayload { deployment_id: deployment_id.clone() };

    let shared_config = aws_config::from_env().load().await; 
    let region_name = shared_config.region().unwrap();

    let client = Client::new(&shared_config);
    let api_function_name = "eventStatusApi";

    let serialized_payload = serde_json::to_vec(&payload)?;
    let payload_blob = Blob::new(serialized_payload);

    info!("Invoking job in region {} using {} with payload: {:?}", region_name, api_function_name, payload);

    let response = client.invoke()
        .function_name(api_function_name)
        .invocation_type(InvocationType::RequestResponse)
        .payload(payload_blob)
        .send().await?;

    let blob = response.payload.unwrap();
    let bytes = blob.into_inner(); // Gets the Vec<u8>
    let response_string = String::from_utf8(bytes)?;
    warn!("Lambda response status: {:?}", response_string);

    let parsed_json: Value = serde_json::from_str(&response_string)?;

    let epoch = parsed_json.get(0)
        .and_then(|val| val.get("epoch").and_then(|e| e.as_i64()))
        .unwrap();

    let status = parsed_json.get(0)
        .and_then(|val| val.get("status").and_then(|s| s.as_str()))
        .unwrap();

    let event = parsed_json.get(0)
        .and_then(|val| val.get("event").and_then(|e| e.as_str()))
        .unwrap()
        .to_string();

    let module = parsed_json.get(0)
        .and_then(|val| val.get("module").and_then(|m| m.as_str()))
        .unwrap()
        .to_string();

    let name = parsed_json.get(0)
        .and_then(|val| val.get("name").and_then(|n| n.as_str()))
        .unwrap()
        .to_string();

    Ok(ApiStatusResult {
        deployment_id: deployment_id.clone(),
        status: status.to_string(),
        epoch: epoch,
        event: event,
        module: module,
        name: name,
    })
}

use aws_config::meta::region::RegionProviderChain;
use aws_sdk_sqs::Client as SqsClient;
use aws_sdk_sns::Client as SnsClient;
use tokio::sync::Mutex;

pub async fn create_queue_and_subscribe_to_topic(sns_topic_arn: String, specs_state: Arc<Mutex<HashMap<String, Value>>>) -> Result<(), Box<dyn std::error::Error>> {
    let region_provider = RegionProviderChain::default_provider().or_else("eu-central-1");
    let config = aws_config::from_env().region(region_provider).load().await;

    // Create SQS client and queue
    let sqs_client = SqsClient::new(&config);
    let create_queue_output = sqs_client.create_queue().queue_name("my-operator-queue").send().await?;
    let queue_url = create_queue_output.queue_url().ok_or("Failed to get queue URL")?;

    // Get the queue ARN
    let get_attrs_response = sqs_client.get_queue_attributes()
        .queue_url(queue_url)
        .set_attribute_names(Some(vec![aws_sdk_sqs::types::QueueAttributeName::QueueArn]))
        .send().await?;

    let queue_arn = get_attrs_response.clone().attributes.as_ref()
        .and_then(|attrs| attrs.get(&QueueAttributeName::QueueArn).cloned())
        .ok_or("Failed to get queue ARN")?
        .to_string();

    // Construct the SQS queue policy that allows SNS to send messages to this queue
    let policy = serde_json::json!({
        "Version": "2012-10-17",
        "Statement": [{
            "Sid": "AllowSNSMessages",
            "Effect": "Allow",
            "Principal": "*",
            "Action": "sqs:SendMessage",
            "Resource": queue_arn,
            "Condition": {
                "ArnEquals": {
                    "aws:SourceArn": sns_topic_arn
                }
            }
        }]
    }).to_string();

    // Set the queue policy
    let mut attributes = std::collections::HashMap::new();
    attributes.insert(QueueAttributeName::Policy, policy);

    sqs_client.set_queue_attributes()
        .queue_url(&queue_url.to_string())
        .set_attributes(Some(attributes)) // Adjusted based on the documentation snippet
        .send().await?;

    // Create SNS client and subscribe the SQS queue to the SNS topic
    let sns_client = SnsClient::new(&config);
    sns_client.subscribe().topic_arn(sns_topic_arn).protocol("sqs").endpoint(queue_arn).send().await?;

    info!("Created queue and subscribed to topic: {}", queue_url);
    
    poll_sqs_messages(queue_url.to_string()).await?;
    Ok(())
}

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use crate::patch::{self, patch_kind};
use crate::FINALIZER_NAME;

async fn poll_sqs_messages(queue_url: String) -> Result<(), Box<dyn std::error::Error>> {
    let region_provider = RegionProviderChain::default_provider().or_else("eu-central-1");
    let config = aws_config::from_env().region(region_provider).load().await;
    let sqs_client = SqsClient::new(&config);

    let kube_client = KubeClient::try_default().await?;

    info!("Polling for messages...");
    loop {
        let received_messages = sqs_client.receive_message()
            .queue_url(&queue_url)
            .wait_time_seconds(20) // Use long polling
            .send().await?;

        // Correctly handle the Option returned by received_messages.messages()
        for message in received_messages.messages.unwrap_or_default() {
            if let Some(body) = message.body() {
                if let Ok(outer_parsed) = serde_json::from_str::<Value>(body) {
                    // Access the "Message" field and parse it as JSON
                    if let Some(inner_message_str) = outer_parsed.get("Message").and_then(|m| m.as_str()) {
                        if let Ok(inner_parsed) = serde_json::from_str::<Value>(inner_message_str) {
                            // Now, extract the deployment_id from the inner JSON
                            if let Some(deployment_id) = inner_parsed.get("deployment_id").and_then(|d| d.as_str()) {
                                info!("Deployment ID: {:?}", deployment_id);

                                warn!("Received message: {:?}", inner_parsed);

                                status_check(deployment_id.to_string());
                            }
                        }
                    }
                }
            }

            debug!("Acking message: {:?}", message.body());

            if let Some(receipt_handle) = message.receipt_handle() {
                sqs_client.delete_message()
                    .queue_url(&queue_url)
                    .receipt_handle(receipt_handle)
                    .send().await?;
            }
        }

        tokio::time::sleep(Duration::from_secs(1)).await; // Sleep to prevent constant polling if no messages are available
    }
}

pub fn status_check(deployment_id: String) {
    // Schedule a status check
    info!("Fetching status for event with deployment_id {}...", deployment_id);

    // Spawn a new asynchronous task for the delayed job
    tokio::spawn(async move {
        let status_json = match read_status(deployment_id.clone()).await {
            Ok(status) => status,
            Err(e) => {
                error!("Failed to read status: {:?}", e);
                return;
            },
        };
        warn!("Status fetched for deployment_id: {:?}", status_json);
        // Read json from status
        info!("Would patch status for deployment_id: {} with {:?}", status_json.deployment_id, status_json);

        let kube_client = KubeClient::try_default().await.unwrap();
        
        // Get the current time in UTC
        let now: DateTime<Utc> = Utc::now();
        // Format the timestamp to RFC 3339 without microseconds
        let timestamp = now.format("%Y-%m-%dT%H:%M:%S%:z").to_string();
        let info = format!("{}: {}", status_json.event, status_json.status);
        // If status_json.status is any of "received", "initiated", set in-progress to true
        let in_progress = if status_json.status == "received" || status_json.status == "initiated" {
            "true"
        } else {
            "false"
        };
        patch_kind(
            kube_client.clone(),
            deployment_id.clone(),
            status_json.module.clone(),
            status_json.name.clone(),
            status_json.module.clone().to_lowercase() + "s",
            "default".to_string(),
            serde_json::json!({
                "metadata": {
                    "annotations": {
                        "in-progress": in_progress,
                    }
                },
                "status": {
                    "resourceStatus": info,
                    "lastStatusUpdate": timestamp,
                }
            })
        ).await;

        if status_json.event == "destroy" && status_json.status == "finished" {
            delete_kind_finalizer(kube_client, status_json.module.clone(), status_json.name, status_json.module.to_lowercase() + "s", "default".to_string()).await;
        } else{
            info!("Not deleting finalizer for: kind: {}, name: {}, plural: {}, namespace: {}", status_json.module, status_json.name, status_json.module.to_lowercase() + "s", "default");
        }
    });
}

async fn delete_kind_finalizer(
    client: KubeClient,
    kind: String,
    name: String,
    plural: String,
    namespace: String,
) {
    warn!("Deleting kind finalizer for: kind: {}, name: {}, plural: {}, namespace: {}", &kind, name, plural, namespace);
    let api_resource = ApiResource::from_gvk_with_plural(
        &GroupVersionKind {
            group: "infrabridge.io".into(),
            version: "v1".into(),
            kind: kind.clone(),
        }, 
        &plural
    );
    let api: Api<DynamicObject> = Api::namespaced_with(client, &namespace, &api_resource);

    let resource = api.get(&name).await;
    match resource {
        Ok(res) => {
            let finalizers = res.metadata.finalizers.unwrap_or_default();
            let finalizers_to_keep: Vec<String> = finalizers.into_iter().filter(|f| f != FINALIZER_NAME).collect();

            warn!("Finalizers after removing {}: {:?}", FINALIZER_NAME, finalizers_to_keep);

            let patch = serde_json::json!({
                "metadata": {
                    "finalizers": finalizers_to_keep,
                    "resourceVersion": res.metadata.resource_version,
                }
            });

            let params = kube::api::PatchParams::default();
            match api.patch(&name, &params, &kube::api::Patch::Merge(&patch)).await {
                Ok(_) => warn!("Finalizer removed for: kind: {}, name: {}, plural: {}, namespace: {}", &kind, name, plural, namespace),
                Err(e) => warn!("Error deleting finalizer: {}", e)
            }
        },
        Err(e) => warn!("Error fetching resource: {}", e),
    }

}
