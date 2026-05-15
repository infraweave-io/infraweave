use env_common::interface::{initialize_project_id_and_region, GenericCloudHandler};
use env_defs::CloudProvider;
use std::env;
use std::future::Future;
use std::path::PathBuf;
use std::process::Command;
use std::sync::OnceLock;
use testcontainers::core::{IntoContainerPort, WaitFor};
use testcontainers::ContainerAsync;
use testcontainers::{runners::AsyncRunner, GenericImage, ImageExt};
use testcontainers_modules::dynamodb_local::DynamoDb;
use testcontainers_modules::localstack::LocalStack;

pub const DYNAMODB_IMAGE: &str = "amazon/dynamodb-local";
pub const MINIO_IMAGE: &str = "minio/minio";
pub const ALL_IMAGES: &[&str] = &[DYNAMODB_IMAGE, MINIO_IMAGE];
pub const SHARED_TEST_NETWORK: &str = "infraweave-integration-tests";

static SHARED_NETWORK_CHECK: OnceLock<()> = OnceLock::new();

#[derive(Clone)]
pub struct TestContext {
    pub api_endpoint: String,
    pub bootstrap_endpoint: String,
    pub api_handler: GenericCloudHandler,
    pub bootstrap_handler: GenericCloudHandler,
}

impl TestContext {
    async fn new(api_endpoint: String, bootstrap_endpoint: String) -> Self {
        let api_handler = GenericCloudHandler::custom(&api_endpoint).await;
        let bootstrap_handler = GenericCloudHandler::custom(&bootstrap_endpoint).await;

        Self {
            api_endpoint,
            bootstrap_endpoint,
            api_handler,
            bootstrap_handler,
        }
    }
}

/// Returns the path to the integration-tests directory (resolved at compile time).
pub fn integration_tests_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn get_image_name(original_image: &str, tag: &str) -> (String, String) {
    let registry_prefix = env::var("DOCKER_IMAGE_MIRROR").unwrap_or_default();

    if registry_prefix.is_empty() {
        return (original_image.to_string(), tag.to_string());
    }

    let mirrored_name = match original_image {
        "public.ecr.aws/lambda/python" => "lambda-python",
        "mcr.microsoft.com/azure-functions/python" => "azure-functions-python",
        "mcr.microsoft.com/cosmosdb/linux/azure-cosmos-emulator" => "azure-cosmos-emulator",
        MINIO_IMAGE => "minio",
        "mcr.microsoft.com/azure-storage/azurite" => "azurite",
        DYNAMODB_IMAGE => "dynamodb-local",
        "localstack/localstack" => "localstack",
        _ => return (original_image.to_string(), tag.to_string()),
    };

    let full_image = format!("{}/{}", registry_prefix, mirrored_name);
    (full_image, tag.to_string())
}

pub async fn test_scaffold<F, Fut>(function_to_test: F)
where
    F: FnOnce(TestContext) -> Fut,
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
    F: FnOnce(TestContext) -> Fut,
    Fut: Future<Output = ()>,
{
    let network = shared_test_network();
    let test_id = generate_random_test_id();
    let dynamodb_container_name = service_container_name(&test_id, "dynamodb");
    let minio_container_name = service_container_name(&test_id, "minio");

    // Start LocalStack for Terraform provider testing (independent of control plane)
    let (_localstack, localstack_endpoint) = start_local_localstack(network).await;
    env::set_var("AWS_ENDPOINT_URL", &localstack_endpoint);
    println!("LocalStack started at: {}", localstack_endpoint);

    // Start DynamoDB locally
    let (_db, dynamodb_endpoint) = start_local_dynamodb(network, &dynamodb_container_name).await;
    let (_minio, minio_host_endpoint) = start_local_minio(network, &minio_container_name).await;

    // Container endpoints for services on Docker network (use container names)
    let minio_container_endpoint = format!("http://{}:9000", minio_container_name);
    let dynamodb_container_endpoint = format!("http://{}:8000", dynamodb_container_name);

    // Set region for local development (required by direct DB access)
    env::set_var("AWS_REGION", "us-west-2");

    // Set MinIO endpoint and credentials for Terraform backend state storage in test mode
    env::set_var("MINIO_ENDPOINT", &minio_host_endpoint);
    env::set_var("MINIO_ACCESS_KEY", "minio");
    env::set_var("MINIO_SECRET_KEY", "minio123");
    env::set_var("DYNAMODB_ENDPOINT", &dynamodb_endpoint);
    println!(
        "MinIO started at: {} (host), {} (containers)",
        minio_host_endpoint, minio_container_endpoint
    );
    println!(
        "DynamoDB started at: {} (host), {} (containers)",
        dynamodb_endpoint, dynamodb_container_endpoint
    );

    let (_lambda_8081, bootstrap_endpoint) = start_lambda(
        network,
        &dynamodb_container_endpoint,
        &minio_container_endpoint,
        &minio_host_endpoint,
    )
    .await;

    let (_lambda_8080, api_endpoint) = start_lambda(
        network,
        &dynamodb_container_endpoint,
        &minio_container_endpoint,
        &minio_host_endpoint,
    )
    .await;
    tokio::time::sleep(std::time::Duration::from_secs(5)).await; // TODO: Find a better way to wait for the lambda to start

    initialize_project_id_and_region().await;
    let context = TestContext::new(api_endpoint, bootstrap_endpoint).await;
    bootstrap_tables(&context).await;
    bootstrap_buckets(&context).await;

    // Perform function tests here
    function_to_test(context).await;
}

pub async fn test_scaffold_azure<F, Fut>(function_to_test: F)
where
    F: FnOnce(TestContext) -> Fut,
    Fut: Future<Output = ()>,
{
    let network = shared_test_network();
    let test_id = generate_random_test_id();
    let cosmos_container_name = service_container_name(&test_id, "cosmos");
    let azurite_container_name = service_container_name(&test_id, "azurite");

    let _cosmos = start_local_cosmosdb(network, &cosmos_container_name).await;
    let (_azurite, azurite_host_connection_string, azurite_container_connection_string) =
        start_local_azurite(network, &azurite_container_name).await;

    // Start LocalStack for AWS provider in test modules
    let (_localstack, localstack_endpoint) = start_local_localstack(network).await;
    env::set_var("AWS_ENDPOINT_URL", &localstack_endpoint);

    env::set_var(
        "AZURE_STORAGE_CONNECTION_STRING",
        &azurite_host_connection_string,
    );
    env::set_var("ARM_SKIP_PROVIDER_REGISTRATION", "true");
    env::set_var("AZURE_HTTP_USER_AGENT", "azurite-test");

    let cosmos_container_endpoint = format!("http://{}:8081", cosmos_container_name);

    let (_azure_8080, api_endpoint) = start_azure_function(
        network,
        &cosmos_container_endpoint,
        &azurite_container_connection_string,
        &azurite_host_connection_string,
    )
    .await;

    let (_azure_8081, bootstrap_endpoint) = start_azure_function(
        network,
        &cosmos_container_endpoint,
        &azurite_container_connection_string,
        &azurite_host_connection_string,
    )
    .await;

    // Set Azure authentication to use environment variables instead of Azure CLI for Terraform
    env::set_var("ARM_USE_CLI", "false");
    env::set_var("ARM_USE_MSI", "false");
    env::set_var("ARM_USE_OIDC", "false");
    // Use static credentials for test mode
    env::set_var("ARM_CLIENT_ID", "00000000-0000-0000-0000-000000000000");
    env::set_var("ARM_CLIENT_SECRET", "fake-secret");
    env::set_var("ARM_TENANT_ID", "00000000-0000-0000-0000-000000000000");
    env::set_var("ARM_SUBSCRIPTION_ID", "dummy-account-id");

    // Set container group name as job ID
    env::set_var("CONTAINER_GROUP_NAME", "running-test-job-id");

    // Using same AWS tf-module as AWS tests (inside Azure runner), which requires following:
    // AWS credentials needed for modules that use AWS providers
    env::set_var("AWS_ACCESS_KEY_ID", "test");
    env::set_var("AWS_SECRET_ACCESS_KEY", "test");
    // Region must be set since its not set in AWS tf-module
    env::set_var("AWS_REGION", "us-west-2");

    initialize_project_id_and_region().await;
    let context = TestContext::new(api_endpoint, bootstrap_endpoint).await;
    bootstrap_tables(&context).await;
    bootstrap_buckets(&context).await;

    // Perform function tests here
    function_to_test(context).await;
}

pub async fn start_lambda(
    network: &str,
    dynamodb_endpoint: &str,
    minio_endpoint: &str,
    minio_host_endpoint: &str,
) -> (ContainerAsync<GenericImage>, String) {
    let base_dir = integration_tests_dir();
    let lambda_source = base_dir.join("lambda-code/test-api.py");
    let bootstrap_source = base_dir.join("lambda-code/bootstrap.py");

    let container_port = 8080;
    let (image_name, image_tag) = get_image_name("public.ecr.aws/lambda/python", "3.11");
    let container = GenericImage::new(&image_name, &image_tag)
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
        .with_env_var("MINIO_HOST_ENDPOINT", minio_host_endpoint)
        .with_env_var("MINIO_ACCESS_KEY", "minio")
        .with_env_var("MINIO_SECRET_KEY", "minio123")
        .with_env_var("MODULE_S3_BUCKET", "modules")
        .with_env_var("POLICY_S3_BUCKET", "policies")
        .with_env_var("CHANGE_RECORD_S3_BUCKET", "change-records")
        .with_env_var("PROVIDERS_S3_BUCKET", "providers")
        .start()
        .await
        .expect("Failed to start lambda");
    let lambda_host_port = container.get_host_port_ipv4(container_port).await.unwrap();
    let lambda_url = format!("http://127.0.0.1:{}", lambda_host_port);
    (container, lambda_url)
}

pub async fn start_azure_function(
    network: &str,
    cosmos_endpoint: &str,
    azurite_connection_string: &str,
    azurite_host_connection_string: &str,
) -> (ContainerAsync<GenericImage>, String) {
    let base_dir = integration_tests_dir();
    let container_port = 80;

    let (image_name, image_tag) = get_image_name("mcr.microsoft.com/azure-functions/python", "4.0");
    let image = GenericImage::new(&image_name, &image_tag)
        .with_exposed_port(container_port.tcp())
        .with_wait_for(WaitFor::message_on_stdout("Application started. Press Ctrl+C to shut down."))
        .with_copy_to("/home/site/wwwroot", base_dir.join("azure-function-code"))
        .with_env_var("AzureWebJobsStorage", "UseDevelopmentStorage=true")
        .with_env_var("AzureWebJobsScriptRoot", "/home/site/wwwroot")
        .with_env_var("FUNCTIONS_WORKER_RUNTIME", "python")
        .with_env_var("COSMOS_DB_ENDPOINT", cosmos_endpoint)
        .with_env_var("COSMOS_DB_DATABASE", "iw_database")
        .with_env_var("AZURITE_CONNECTION_STRING", azurite_connection_string)
        .with_env_var("AZURITE_HOST_ENDPOINT", azurite_host_endpoint_from_connection_string(azurite_host_connection_string))
        .with_env_var("COSMOS_KEY", "C2y6yDjf5/R+ob0N8A7Cgv30VRDJIWEHLM+4QDU5DE2nQ9nDuVTqobD4b8mGGyPMbIZnqyMsEcaGQy67XIw/Jw==")
        .with_cmd(vec![
            "/bin/bash",
            "-c",
            "pip install -r /home/site/wwwroot/requirements.txt && /azure-functions-host/Microsoft.Azure.WebJobs.Script.WebHost",
        ])
        .with_network(network);

    let container = image
        .start()
        .await
        .expect("Failed to start Azure Functions container");

    let function_host_port = container.get_host_port_ipv4(container_port).await.unwrap();
    let function_url = format!("http://127.0.0.1:{}", function_host_port);

    (container, function_url)
}

pub async fn start_local_dynamodb(
    network: &str,
    container_name: &str,
) -> (ContainerAsync<DynamoDb>, String) {
    let (image_name, image_tag) = get_image_name(DYNAMODB_IMAGE, "latest");
    let db = DynamoDb::default()
        .with_name(image_name)
        .with_tag(image_tag)
        .with_network(network)
        .with_container_name(container_name)
        .start()
        .await
        .unwrap();

    let dynamodb_host_port = db.get_host_port_ipv4(8000).await.unwrap();
    let dynamodb_endpoint = format!("http://127.0.0.1:{}", dynamodb_host_port);
    (db, dynamodb_endpoint)
}

pub async fn start_local_cosmosdb(
    network: &str,
    container_name: &str,
) -> ContainerAsync<GenericImage> {
    let container_port = 8081;

    let (image_name, image_tag) = get_image_name(
        "mcr.microsoft.com/cosmosdb/linux/azure-cosmos-emulator",
        "vnext-preview",
    );
    let image = GenericImage::new(&image_name, &image_tag)
        .with_exposed_port(container_port.tcp())
        .with_env_var("AZURE_COSMOS_EMULATOR_PARTITION_COUNT", "1")
        .with_env_var("AZURE_COSMOS_EMULATOR_ENABLE_DATA_PERSISTENCE", "true")
        .with_env_var("ENABLE_EXPLORER", "false")
        .with_env_var("PROTOCOL", "http")
        .with_env_var("LOG_LEVEL", "trace")
        .with_network(network);

    let container = image
        .with_container_name(container_name.to_string())
        .start()
        .await
        .expect("Failed to start local Cosmos DB Emulator");

    container
}

pub async fn start_local_minio(
    network: &str,
    container_name: &str,
) -> (ContainerAsync<GenericImage>, String) {
    let (image_name, image_tag) = get_image_name(MINIO_IMAGE, "latest");
    let minio = GenericImage::new(&image_name, &image_tag)
        .with_exposed_port(9000.tcp())
        .with_network(network)
        .with_container_name(container_name)
        .with_env_var("MINIO_ACCESS_KEY", "minio")
        .with_env_var("MINIO_SECRET_KEY", "minio123")
        .with_cmd(["server", "/data"])
        .start()
        .await
        .expect("Failed to start minio");

    let minio_host_port = minio.get_host_port_ipv4(9000).await.unwrap();
    let minio_host_endpoint = format!("http://127.0.0.1:{}", minio_host_port);

    (minio, minio_host_endpoint)
}

pub async fn start_local_azurite(
    network: &str,
    container_name: &str,
) -> (ContainerAsync<GenericImage>, String, String) {
    let azurite_blob_port = 10000.tcp();

    let (image_name, image_tag) =
        get_image_name("mcr.microsoft.com/azure-storage/azurite", "latest");
    let image = GenericImage::new(&image_name, &image_tag)
        .with_exposed_port(azurite_blob_port)
        .with_wait_for(WaitFor::message_on_stdout(
            "Azurite Blob service is successfully listening at",
        ))
        .with_network(network);

    let container = image
        .with_container_name(container_name.to_string())
        .start()
        .await
        .expect("Failed to start Azurite container");

    let actual_mapped_port = container
        .get_host_port_ipv4(azurite_blob_port)
        .await
        .expect("Failed to get mapped Azurite port");

    let azurite_host_endpoint = format!("http://127.0.0.1:{}", actual_mapped_port);
    let azurite_host_connection_string = format!(
        "DefaultEndpointsProtocol=http;AccountName=devstoreaccount1;AccountKey=Eby8vdM02xNOcqFlqUwJPLlmEtlCDXJ1OUzFT50uSRZ6IFsuFq2UVErCz4I6tq/K1SZFPTOtr/KBHBeksoGMGw==;BlobEndpoint={};",
        azurite_host_endpoint
    );

    let azurite_container_endpoint = format!("http://{}:10000", container_name);
    let azurite_container_connection_string = format!(
        "DefaultEndpointsProtocol=http;AccountName=devstoreaccount1;AccountKey=Eby8vdM02xNOcqFlqUwJPLlmEtlCDXJ1OUzFT50uSRZ6IFsuFq2UVErCz4I6tq/K1SZFPTOtr/KBHBeksoGMGw==;BlobEndpoint={}/devstoreaccount1;",
        azurite_container_endpoint
    );

    (
        container,
        azurite_host_connection_string,
        azurite_container_connection_string,
    )
}

pub async fn bootstrap_tables(context: &TestContext) {
    let payload = serde_json::json!({ "event": "bootstrap_tables" });
    context
        .bootstrap_handler
        .run_function(&payload)
        .await
        .unwrap();
}

pub async fn bootstrap_buckets(context: &TestContext) {
    let payload = serde_json::json!({ "event": "bootstrap_buckets" });
    context.api_handler.run_function(&payload).await.unwrap();
}

pub fn generate_random_test_id() -> String {
    let random_id: u32 = rand::random();
    format!("test-{}", random_id)
}

pub fn shared_test_network() -> &'static str {
    SHARED_NETWORK_CHECK.get_or_init(|| {
        let inspect_output = Command::new("docker")
            .args(["network", "inspect", SHARED_TEST_NETWORK])
            .output()
            .expect("Failed to inspect Docker integration test network");

        if inspect_output.status.success() {
            return;
        }

        let create_output = Command::new("docker")
            .args(["network", "create", SHARED_TEST_NETWORK])
            .output()
            .expect("Failed to create Docker integration test network");

        let create_stderr = String::from_utf8_lossy(&create_output.stderr);
        if !create_output.status.success() && !create_stderr.contains("already exists") {
            let inspect_stderr = String::from_utf8_lossy(&inspect_output.stderr);
            panic!(
                "Failed to ensure Docker network '{SHARED_TEST_NETWORK}' exists.\n\
                 docker network inspect stderr: {}\n\
                 docker network create stderr: {}",
                inspect_stderr.trim(),
                create_stderr.trim()
            );
        }
    });

    SHARED_TEST_NETWORK
}

pub fn service_container_name(test_id: &str, service: &str) -> String {
    format!("{service}-{test_id}")
}

fn azurite_host_endpoint_from_connection_string(connection_string: &str) -> String {
    connection_string
        .split(';')
        .find_map(|part| part.strip_prefix("BlobEndpoint="))
        .unwrap_or("http://127.0.0.1:10000")
        .to_string()
}

pub async fn start_local_localstack(network: &str) -> (ContainerAsync<LocalStack>, String) {
    let (image_name, image_tag) = get_image_name("localstack/localstack", "3.0");
    let localstack = LocalStack::default()
        .with_name(image_name)
        .with_tag(image_tag)
        .with_network(network)
        .start()
        .await
        .unwrap();

    let localstack_host_port = localstack.get_host_port_ipv4(4566).await.unwrap();
    let localstack_endpoint = format!("http://127.0.0.1:{}", localstack_host_port);

    (localstack, localstack_endpoint)
}

#[allow(dead_code)]
pub async fn upload_file(
    handler: &GenericCloudHandler,
    key: &String,
    file_path: &String,
) -> Result<(), anyhow::Error> {
    use base64::engine::general_purpose::STANDARD as base64_engine;
    use base64::Engine;

    let file_content = std::fs::read(file_path)
        .map_err(|e| anyhow::anyhow!("Failed to read file {}: {}", file_path, e))?;
    let zip_base64 = base64_engine.encode(file_content);

    let payload = env_defs::upload_file_base64_event(key, "modules", &zip_base64);
    match handler.run_function(&payload).await {
        Ok(_) => {
            println!("Successfully uploaded module zip file to S3");
            Ok(())
        }
        Err(error) => Err(anyhow::anyhow!("{}", error)),
    }
}
