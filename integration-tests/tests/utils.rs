use env_common::interface::{initialize_project_id_and_region, GenericCloudHandler};

use env_defs::CloudProvider;
use rand::RngCore;
use std::env;
use std::future::Future;
use testcontainers::core::{IntoContainerPort, WaitFor};
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
    if env::var("PROVIDER").unwrap_or("azure".to_string()) == "azure" {
        test_scaffold_azure(function_to_test).await;
    } else {
        test_scaffold_aws(function_to_test).await;
    }
}

pub async fn test_scaffold_aws<F, Fut>(function_to_test: F)
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = ()>,
{
    let network = generate_random_network_name();

    // Start DynamoDB locally

    let (_db, dynamodb_endpoint) = start_local_dynamodb(&network, 8000).await;
    let (_minio, minio_endpoint) = start_local_minio(&network, 9000).await;
    let _lambda_8081 = start_lambda(&network, &dynamodb_endpoint, &minio_endpoint, 8081).await;
    let _lambda_8080 = start_lambda(&network, &dynamodb_endpoint, &minio_endpoint, 8080).await;
    tokio::time::sleep(std::time::Duration::from_secs(5)).await; // TODO: Find a better way to wait for the lambda to start

    initialize_project_id_and_region().await;
    bootstrap_tables().await;
    bootstrap_buckets().await;

    // Perform function tests here
    function_to_test().await;
}

pub async fn test_scaffold_azure<F, Fut>(function_to_test: F)
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = ()>,
{
    let network = generate_random_network_name();

    let _cosmos = start_local_cosmosdb(&network, 8000).await;
    let (_azurite, azurite_connection_string) = start_local_azurite(&network, 10000).await;
    let _azure_8080: ContainerAsync<GenericImage> = start_azure_function(
        &network,
        "http://cosmos:8081",
        &azurite_connection_string,
        8080,
    )
    .await;
    let _azure_8081: ContainerAsync<GenericImage> = start_azure_function(
        &network,
        "http://cosmos:8081",
        &azurite_connection_string,
        8081,
    )
    .await;

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
        .with_env_var("DYNAMODB_CONFIG_TABLE_NAME", "config")
        .with_env_var("MINIO_ENDPOINT", minio_endpoint)
        .with_env_var("MINIO_ACCESS_KEY", "minio")
        .with_env_var("MINIO_SECRET_KEY", "minio123")
        .with_env_var("MODULE_S3_BUCKET", "modules")
        .with_env_var("POLICY_S3_BUCKET", "policies")
        .with_env_var("CHANGE_RECORD_S3_BUCKET", "change-records")
        .with_env_var("PROVIDERS_S3_BUCKET", "providers")
        .with_mapped_port(port, container_port.tcp())
        .start()
        .await
        .expect("Failed to start lambda");
    let lambda_host_port = container.get_host_port_ipv4(container_port).await.unwrap();
    let lambda_url = format!("http://127.0.0.1:{}", lambda_host_port);
    std::env::set_var("LAMBDA_ENDPOINT_URL", &lambda_url);
    return container;
}

pub async fn start_azure_function(
    network: &str,
    cosmos_endpoint: &str,
    azurite_connection_string: &str,
    port: u16,
) -> ContainerAsync<GenericImage> {
    let current_dir = env::current_dir().expect("Failed to get current directory");
    let container_port = 80;

    let image = GenericImage::new("mcr.microsoft.com/azure-functions/python", "4.0")
        .with_exposed_port(container_port.tcp())
        .with_wait_for(WaitFor::message_on_stdout("Application started. Press Ctrl+C to shut down."))
        .with_copy_to("/home/site/wwwroot", current_dir.join("azure-function-code"))
        .with_env_var("AzureWebJobsStorage", "UseDevelopmentStorage=true")
        .with_env_var("AzureWebJobsScriptRoot", "/home/site/wwwroot")
        .with_env_var("FUNCTIONS_WORKER_RUNTIME", "python")
        .with_env_var("COSMOS_DB_ENDPOINT", cosmos_endpoint)
        .with_env_var("COSMOS_DB_DATABASE", "iw_database")
        .with_env_var("AZURITE_CONNECTION_STRING", azurite_connection_string)
        .with_env_var("COSMOS_KEY", "C2y6yDjf5/R+ob0N8A7Cgv30VRDJIWEHLM+4QDU5DE2nQ9nDuVTqobD4b8mGGyPMbIZnqyMsEcaGQy67XIw/Jw==")
        .with_cmd(vec![
            "/bin/bash",
            "-c",
            "pip install -r /home/site/wwwroot/requirements.txt && /azure-functions-host/Microsoft.Azure.WebJobs.Script.WebHost",
        ])
        .with_network(network);

    let container = image
        .with_mapped_port(port, container_port.tcp())
        .start()
        .await
        .expect("Failed to start Azure Functions container");

    container
}

pub async fn start_local_dynamodb(network: &str, port: u16) -> (ContainerAsync<DynamoDb>, String) {
    let db = DynamoDb::default()
        .with_network(network)
        .with_mapped_port(port, 8000.tcp())
        .start()
        .await
        .unwrap();

    let dynamodb_host_port = db.get_host_port_ipv4(8000).await.unwrap();
    let dynamodb_endpoint = format!(
        "http://{}:{}",
        db.get_bridge_ip_address().await.unwrap(),
        dynamodb_host_port
    );
    (db, dynamodb_endpoint)
}

pub async fn start_local_cosmosdb(network: &str, port: u16) -> ContainerAsync<GenericImage> {
    let container_port = 8081;

    let image = GenericImage::new(
        "mcr.microsoft.com/cosmosdb/linux/azure-cosmos-emulator",
        "vnext-preview",
    )
    .with_exposed_port(container_port.tcp())
    .with_env_var("AZURE_COSMOS_EMULATOR_PARTITION_COUNT", "1")
    .with_env_var("AZURE_COSMOS_EMULATOR_ENABLE_DATA_PERSISTENCE", "true")
    .with_env_var("ENABLE_EXPLORER", "false")
    .with_env_var("PROTOCOL", "http")
    .with_env_var("LOG_LEVEL", "trace")
    .with_network(network);

    let container = image
        .with_container_name("cosmos".to_string())
        .with_mapped_port(port, container_port.tcp())
        .start()
        .await
        .expect("Failed to start local Cosmos DB Emulator");

    container
}

pub async fn start_local_minio(network: &str, port: u16) -> (ContainerAsync<GenericImage>, String) {
    let minio = GenericImage::new("minio/minio", "latest")
        .with_network(network)
        .with_env_var("MINIO_ACCESS_KEY", "minio")
        .with_env_var("MINIO_SECRET_KEY", "minio123")
        .with_cmd(["server", "/data"])
        .with_mapped_port(port, 9000.tcp())
        .start()
        .await
        .expect("Failed to start minio");

    let minio_host_port = minio.get_host_port_ipv4(9000).await.unwrap();
    let minio_ip = minio.get_bridge_ip_address().await.unwrap();
    let minio_endpoint = format!("http://{}:{}", minio_ip, minio_host_port);

    (minio, minio_endpoint)
}

pub async fn start_local_azurite(
    network: &str,
    host_port: u16,
) -> (ContainerAsync<GenericImage>, String) {
    let azurite_blob_port = 10000.tcp();

    let image = GenericImage::new("mcr.microsoft.com/azure-storage/azurite", "latest")
        .with_exposed_port(azurite_blob_port)
        .with_wait_for(WaitFor::message_on_stdout(
            "Azurite Blob service is successfully listening at",
        ))
        .with_env_var("AZURITE_ACCOUNTS", "storageAccount1:bW9kdWxlc2tleQ==")
        .with_network(network);

    let container = image
        .with_container_name("azurite".to_string())
        .with_mapped_port(host_port, azurite_blob_port)
        .start()
        .await
        .expect("Failed to start Azurite container");

    let actual_mapped_port = container
        .get_host_port_ipv4(azurite_blob_port)
        .await
        .expect("Failed to get mapped Azurite port");

    let azurite_blob_endpoint = format!("http://azurite:{}/storageAccount1", actual_mapped_port);
    let azurite_connection_string = format!(
        "DefaultEndpointsProtocol=http;AccountName=storageAccount1;AccountKey=bW9kdWxlc2tleQ==;BlobEndpoint={};",
        azurite_blob_endpoint
    );

    (container, azurite_connection_string)
}

pub async fn bootstrap_tables() {
    let payload = serde_json::json!({ "event": "bootstrap_tables" });
    let function_endpoint_url = "http://127.0.0.1:8081";
    GenericCloudHandler::custom(function_endpoint_url)
        .await
        .run_function(&payload)
        .await
        .unwrap();
}

pub async fn bootstrap_buckets() {
    let payload = serde_json::json!({ "event": "bootstrap_buckets" });
    let function_endpoint_url = "http://127.0.0.1:8080";
    GenericCloudHandler::custom(function_endpoint_url)
        .await
        .run_function(&payload)
        .await
        .unwrap();
}

pub fn generate_random_network_name() -> String {
    let mut rng = rand::thread_rng();
    let random_id: u32 = rng.next_u32();
    format!("testcontainers-network-{}", random_id)
}
