use aws_sdk_sqs::types::QueueAttributeName;
use log::{error, info, warn};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
struct ApiStatusLambdaPayload {
    deployment_id: String,
    #[serde(rename = "type")]
    type_: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct ApiStatusLogLambdaPayload {
    job_id: String,
    #[serde(rename = "type")]
    type_: String,
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

pub async fn read_status(
    deployment_id: String,
) -> Result<ApiStatusResult, Box<dyn std::error::Error>> {
    let events = crate::api_event::get_events(&deployment_id).await?;
    let event = events.last().unwrap();

    Ok(ApiStatusResult {
        deployment_id: deployment_id.clone(),
        status: event.clone().status,
        epoch: event.clone().epoch as i64,
        event: event.clone().event,
        module: event.clone().module,
        name: event.clone().name,
        job_id: event.clone().job_id,
    })
}

pub async fn read_logs(job_id: &str) -> Result<std::string::String, anyhow::Error> {
    println!("Reading logs for job_id: {}", job_id);

    let payload = ApiStatusLogLambdaPayload {
        job_id: job_id.to_string(),
        type_: "logs".to_string(),
    };

    let payload = serde_json::json!({
        "event": "read_logs",
        "data": payload
    });

    let response = match run_lambda(payload).await {
        Ok(response) => response,
        Err(e) => {
            error!("Failed to read logs: {}", e);
            println!("Failed to read logs: {}", e);
            return Err(anyhow::anyhow!("Failed to read logs: {}", e));
        }
    };

    let log_events = response.get("events").expect("Log events not found");

    if let Some(log_entries) = log_events.as_array() {
        let mut log_entry_vec: Vec<String> = vec![];
        for log in log_entries {
            // warn!("Event: {:?}", event);
            let message = log
                .get("message")
                .expect("message not found")
                .as_str()
                .expect("message not a string")
                .to_string();
            log_entry_vec.push(message);
        }

        let logs = log_entry_vec.join("\n");
        println!("Logs: {}", logs);
        return Ok(logs);
    } else {
        panic!("Expected an array of log_entry");
    }
}

use aws_config::meta::region::RegionProviderChain;
use aws_sdk_sns::Client as SnsClient;
use aws_sdk_sqs::Client as SqsClient;

use crate::api::run_lambda;

pub async fn create_queue_and_subscribe_to_topic(
    sns_topic_arn: String,
) -> Result<String, Box<dyn std::error::Error>> {
    let region_provider = RegionProviderChain::default_provider().or_else("eu-central-1");
    let config = aws_config::from_env().region(region_provider).load().await;

    // Create SQS client and queue
    let sqs_client = SqsClient::new(&config);
    let create_queue_output = sqs_client
        .create_queue()
        .queue_name("my-operator-queue")
        .send()
        .await?;
    let queue_url = create_queue_output
        .queue_url()
        .ok_or("Failed to get queue URL")?;

    // Get the queue ARN
    let get_attrs_response = sqs_client
        .get_queue_attributes()
        .queue_url(queue_url)
        .set_attribute_names(Some(vec![aws_sdk_sqs::types::QueueAttributeName::QueueArn]))
        .send()
        .await?;

    let queue_arn = get_attrs_response
        .clone()
        .attributes
        .as_ref()
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
    })
    .to_string();

    // Set the queue policy
    let mut attributes = std::collections::HashMap::new();
    attributes.insert(QueueAttributeName::Policy, policy);

    sqs_client
        .set_queue_attributes()
        .queue_url(&queue_url.to_string())
        .set_attributes(Some(attributes)) // Adjusted based on the documentation snippet
        .send()
        .await?;

    // Create SNS client and subscribe the SQS queue to the SNS topic
    let sns_client = SnsClient::new(&config);
    sns_client
        .subscribe()
        .topic_arn(sns_topic_arn)
        .protocol("sqs")
        .endpoint(queue_arn)
        .send()
        .await?;

    info!("Created queue and subscribed to topic: {}", queue_url);

    Ok(queue_url.to_string())
}
