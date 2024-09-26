use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct TaskMetadata {
    #[serde(rename = "TaskARN")]
    task_arn: String,
}

use reqwest::Client;
use std::env;

// TODO: to be moved to environment specific module
pub async fn get_job_id() -> Result<String, Box<dyn std::error::Error>> {
    let metadata_uri = env::var("ECS_CONTAINER_METADATA_URI_V4")
        .or_else(|_| env::var("ECS_CONTAINER_METADATA_URI"))
        .expect("ECS metadata URI not found in environment variables");

    let task_metadata_url = format!("{}/task", metadata_uri);

    let client = Client::new();
    let response = client.get(&task_metadata_url).send().await?;
    if !response.status().is_success() {
        panic!("Failed to get task metadata: HTTP {}", response.status());
    }

    let task_metadata: TaskMetadata = response.json().await?;
    let task_arn = task_metadata.task_arn;

    println!("Task ARN: {}", task_arn);

    Ok(task_arn)
}
