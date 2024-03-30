use aws_sdk_lambda::{Client, Error as AwsError};
use aws_sdk_lambda::types::InvocationType;
use aws_sdk_lambda::primitives::Blob;
use aws_sdk_sqs::types::QueueAttributeName;
use std::collections::HashMap;
use std::sync::Arc;
use chrono::{DateTime, Utc};
// use kube::api::{ApiResource, DynamicObject, GroupVersionKind};
// use kube::{Api, Client as KubeClient};
use serde::{Serialize, Deserialize};
use log::{debug, error, info, warn};
use serde_json::Value;

#[derive(Debug, Serialize, Deserialize)]
struct ApiStatusLambdaPayload {
    deployment_id: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ApiStatusResult {
    pub deployment_id: String,
    pub status: String,
    pub epoch: i64,
    pub event: String,
    pub module: String,
    pub name: String,
    pub job_id: String,
    // spec: serde_json::value::Value,
    // manifest: serde_json::value::Value,
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

    let job_id = parsed_json.get(0)
        .and_then(|val| val.get("job_id").and_then(|n| n.as_str()))
        .unwrap_or("")
        .to_string();

    Ok(ApiStatusResult {
        deployment_id: deployment_id.clone(),
        status: status.to_string(),
        epoch: epoch,
        event: event,
        module: module,
        name: name,
        job_id: job_id,
    })
}

use aws_config::meta::region::RegionProviderChain;
use aws_sdk_sqs::Client as SqsClient;
use aws_sdk_sns::Client as SnsClient;

pub async fn create_queue_and_subscribe_to_topic(
    sns_topic_arn: String
) -> Result<String, Box<dyn std::error::Error>> {
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
    
    Ok(queue_url.to_string())
}

