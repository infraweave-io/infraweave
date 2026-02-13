use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct TaskMetadata {
    #[serde(rename = "TaskARN")]
    task_arn: String,
}

use reqwest::Client;
use std::env;

pub async fn get_current_job_id() -> Result<String, anyhow::Error> {
    if std::env::var("TEST_MODE").is_ok() {
        return Ok("running-test-job-id".to_string());
    };

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

    eprintln!("Task ARN: {}", task_arn);

    let job_id = task_arn.split('/').next_back().unwrap().to_string();

    Ok(job_id)
}
