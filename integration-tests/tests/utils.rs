use env_common::interface::{initialize_project_id_and_region, CloudHandler};

use rand::RngCore;
use std::env;
use std::future::Future;
use testcontainers::core::IntoContainerPort;
use testcontainers::ContainerAsync;
use testcontainers::{runners::AsyncRunner, GenericImage, ImageExt};
use testcontainers_modules::dynamodb_local::DynamoDb;

// TODO: Enable running tests in parallel
// Currently the tests are run sequentially because the lambda container is started with a fixed port
// using "cargo test -p integration-tests -- --test-threads=1"

pub async fn test_scaffold<F, Fut>(function_to_test: F)
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = ()>,
{
    let network = generate_random_network_name();

    // Start DynamoDB locally

    let db = DynamoDb::default()
        .with_network(&network)
        .with_mapped_port(8000, 8000.tcp())
        .start()
        .await
        .unwrap();

    let dynamodb_host_port = db.get_host_port_ipv4(8000).await.unwrap();
    let dynamodb_endpoint = format!(
        "http://{}:{}",
        db.get_bridge_ip_address().await.unwrap(),
        dynamodb_host_port
    );

    // Start MinIO locally

    let minio = GenericImage::new("minio/minio", "latest")
        .with_network(&network)
        .with_env_var("MINIO_ACCESS_KEY", "minio")
        .with_env_var("MINIO_SECRET_KEY", "minio123")
        .with_cmd(["server", "/data"])
        .with_mapped_port(9000, 9000.tcp())
        .start()
        .await
        .expect("Failed to start minio");

    let minio_host_port = minio.get_host_port_ipv4(9000).await.unwrap();
    let minio_ip = minio.get_bridge_ip_address().await.unwrap();
    let minio_endpoint = format!("http://{}:{}", minio_ip, minio_host_port);

    tokio::time::sleep(std::time::Duration::from_secs(1)).await;

    // Start Lambda locally
    let _lambda_8081 = start_lambda(&network, &dynamodb_endpoint, &minio_endpoint, 8081).await;
    let _lambda_8080 = start_lambda(&network, &dynamodb_endpoint, &minio_endpoint, 8080).await;
    tokio::time::sleep(std::time::Duration::from_secs(5)).await; // TODO: Find a better way to wait for the lambda to start

    initialize_project_id_and_region().await;
    bootstrap_tables().await;
    bootstrap_buckets().await;

    // Perform function tests here
    function_to_test().await;
}

pub async fn start_lambda(
    network: &str,
    dynamodb_endpoint: &str,
    minio_endpoint: &str,
    port: u16,
) -> ContainerAsync<GenericImage> {
    let current_dir = env::current_dir().expect("Failed to get current directory");
    println!("Current directory: {:?}", current_dir);
    let lambda_source = current_dir.join("lambda-code/test-api.py");
    let bootstrap_source = current_dir.join("lambda-code/bootstrap.py");

    let container_port = 8080;

    let container = GenericImage::new("public.ecr.aws/lambda/python", "3.11")
        .with_exposed_port(container_port.tcp())
        .with_copy_to("/var/task/test-api.py", lambda_source)
        .with_copy_to("/var/task/bootstrap.py", bootstrap_source)
        .with_cmd(vec!["test-api.handler"])
        .with_network(network)
        .with_env_var("DEBUG", "1")
        .with_env_var("REGION", "us-west-2")
        .with_env_var("DYNAMODB_ENDPOINT_URL", dynamodb_endpoint)
        .with_env_var("DYNAMODB_EVENTS_TABLE_NAME", "events")
        .with_env_var("DYNAMODB_MODULES_TABLE_NAME", "modules")
        .with_env_var("DYNAMODB_POLICIES_TABLE_NAME", "policies")
        .with_env_var("DYNAMODB_DEPLOYMENTS_TABLE_NAME", "deployments")
        .with_env_var("DYNAMODB_CHANGE_RECORDS_TABLE_NAME", "change-records")
        .with_env_var("MINIO_ENDPOINT", minio_endpoint)
        .with_env_var("MINIO_ACCESS_KEY", "minio")
        .with_env_var("MINIO_SECRET_KEY", "minio123")
        .with_env_var("MODULE_S3_BUCKET", "modules")
        .with_mapped_port(port, container_port.tcp())
        .start()
        .await
        .expect("Failed to start lambda");
    let lambda_host_port = container.get_host_port_ipv4(container_port).await.unwrap();
    let lambda_url = format!("http://127.0.0.1:{}", lambda_host_port);
    std::env::set_var("LAMBDA_ENDPOINT_URL", &lambda_url);
    return container;
}

pub async fn bootstrap_tables() {
    let payload = serde_json::json!({ "event": "bootstrap_tables" });
    let lambda_endpoint_url = "http://127.0.0.1:8081";
    env_common::logic::custom_handler(lambda_endpoint_url)
        .run_function(&payload)
        .await
        .unwrap();
}

pub async fn bootstrap_buckets() {
    let payload = serde_json::json!({ "event": "bootstrap_buckets" });
    let lambda_endpoint_url = "http://127.0.0.1:8080";
    env_common::logic::custom_handler(lambda_endpoint_url)
        .run_function(&payload)
        .await
        .unwrap();
}

pub fn generate_random_network_name() -> String {
    let mut rng = rand::thread_rng();
    let random_id: u32 = rng.next_u32();
    format!("testcontainers-network-{}", random_id)
}
