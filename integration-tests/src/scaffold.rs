use env_common::interface::{initialize_project_id_and_region, GenericCloudHandler};
use env_defs::CloudProvider;
use std::env;
use std::future::Future;
use std::path::PathBuf;
use testcontainers::core::{IntoContainerPort, WaitFor};
use testcontainers::ContainerAsync;
use testcontainers::{runners::AsyncRunner, GenericImage, ImageExt};
use testcontainers_modules::dynamodb_local::DynamoDb;
use testcontainers_modules::localstack::LocalStack;

/// Mirrored images are read from `ghcr.io/<IMAGE_REPOSITORY>` (populated by
/// .github/workflows/image_mirror.yml).
pub const DEFAULT_IMAGE_REPOSITORY: &str = "infraweave-io/infraweave";

/// Images used by the integration tests. Each one must have a matching `to`
/// entry (`name:tag`) in .github/vars/default.image_mirror.json; a unit test
/// below enforces it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MirroredImage {
    DynamoDb,
    Minio,
    LocalStack,
    LambdaPython,
    AzureFunctionsPython,
    CosmosEmulator,
    Azurite,
    K3s,
    /// Trace backend used by the telemetry integration test: accepts OTLP and
    /// exposes a query API to assert on what actually arrived.
    Jaeger,
    /// Used by `run_terraform_provider_lock` via `INFRAWEAVE_TF_IMAGE`.
    OpenTofu,
}

impl MirroredImage {
    pub const ALL: &'static [MirroredImage] = &[
        Self::DynamoDb,
        Self::Minio,
        Self::LocalStack,
        Self::LambdaPython,
        Self::AzureFunctionsPython,
        Self::CosmosEmulator,
        Self::Azurite,
        Self::K3s,
        Self::Jaeger,
        Self::OpenTofu,
    ];

    /// Local backend (DynamoDB + MinIO) started by tests that run without the
    /// full scaffold; used to clean up leftovers from crashed runs.
    pub const LOCAL_BACKEND: &'static [MirroredImage] = &[Self::DynamoDb, Self::Minio];

    /// Image name in the mirror (the part before `:` in the `to` field).
    pub fn name(self) -> &'static str {
        match self {
            Self::DynamoDb => "dynamodb-local",
            Self::Minio => "minio",
            Self::LocalStack => "localstack",
            Self::LambdaPython => "lambda-python",
            Self::AzureFunctionsPython => "azure-functions-python",
            Self::CosmosEmulator => "azure-cosmos-emulator",
            Self::Azurite => "azurite",
            Self::K3s => "k3s",
            Self::Jaeger => "jaeger-all-in-one",
            Self::OpenTofu => "opentofu",
        }
    }

    pub fn tag(self) -> &'static str {
        match self {
            Self::DynamoDb | Self::Minio | Self::Azurite => "latest",
            Self::LocalStack => "3.0",
            Self::LambdaPython => "3.11",
            Self::AzureFunctionsPython => "4.0",
            Self::CosmosEmulator => "vnext-preview",
            Self::K3s => "v1.31.5-k3s1",
            Self::Jaeger => "1.62.0",
            Self::OpenTofu => "1",
        }
    }

    /// `name:tag`, as written in the `to` field of the mirror list.
    pub fn mirror_entry(self) -> String {
        format!("{}:{}", self.name(), self.tag())
    }

    /// Full image name without tag: `ghcr.io/<IMAGE_REPOSITORY>/<name>`.
    pub fn repository(self) -> String {
        format!("ghcr.io/{}/{}", image_repository(), self.name())
    }

    /// Full image reference: `ghcr.io/<IMAGE_REPOSITORY>/<name>:<tag>`.
    pub fn reference(self) -> String {
        format!("{}:{}", self.repository(), self.tag())
    }

    /// A `GenericImage` for this mirrored image, ready for further configuration.
    pub fn generic_image(self) -> GenericImage {
        GenericImage::new(self.repository(), self.tag().to_string())
    }
}

impl std::fmt::Display for MirroredImage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.reference())
    }
}

/// Returns the path to the integration-tests directory (resolved at compile time).
pub fn integration_tests_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// The GHCR repository mirrored images are read from: `IMAGE_REPOSITORY`, or
/// `DEFAULT_IMAGE_REPOSITORY` when unset/empty.
pub fn image_repository() -> String {
    env::var("IMAGE_REPOSITORY")
        .ok()
        .filter(|r| !r.is_empty())
        .unwrap_or_else(|| DEFAULT_IMAGE_REPOSITORY.to_string())
}

/// Point `run_terraform_provider_lock` at the mirrored OpenTofu image, unless
/// `INFRAWEAVE_TF_IMAGE` is already set.
pub fn use_mirrored_tf_image() {
    if env::var("INFRAWEAVE_TF_IMAGE").is_err() {
        env::set_var("INFRAWEAVE_TF_IMAGE", MirroredImage::OpenTofu.reference());
    }
}

pub async fn test_scaffold<F, Fut>(function_to_test: F)
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = ()>,
{
    use_mirrored_tf_image();
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

    // Start LocalStack for Terraform provider testing (independent of control plane)
    let (_localstack, localstack_endpoint) = start_local_localstack(&network, 4566).await;
    env::set_var("AWS_ENDPOINT_URL", &localstack_endpoint);
    println!("LocalStack started at: {}", localstack_endpoint);

    // Start DynamoDB locally
    let (_db, dynamodb_endpoint) = start_local_dynamodb(&network, 8000).await;
    let (_minio, minio_host_endpoint) = start_local_minio(&network, 9000).await;

    // Container endpoints for services on Docker network (use container names)
    let minio_container_endpoint = "http://minio:9000";
    let dynamodb_container_endpoint = "http://dynamodb:8000";

    // Set region for local development (required by direct DB access)
    env::set_var("AWS_REGION", "us-west-2");

    // Set MinIO endpoint and credentials for Terraform backend state storage in test mode
    env::set_var("MINIO_ENDPOINT", &minio_host_endpoint);
    env::set_var("MINIO_ACCESS_KEY", "minio");
    env::set_var("MINIO_SECRET_KEY", "minio123");
    env::set_var("AWS_ACCESS_KEY_ID", "minio");
    env::set_var("AWS_SECRET_ACCESS_KEY", "minio123");
    env::set_var("DYNAMODB_ENDPOINT", &dynamodb_endpoint);
    println!(
        "MinIO started at: {} (host), {} (containers)",
        minio_host_endpoint, minio_container_endpoint
    );
    println!(
        "DynamoDB started at: {} (host), {} (containers)",
        dynamodb_endpoint, dynamodb_container_endpoint
    );

    let _lambda_8081 = start_lambda(
        &network,
        dynamodb_container_endpoint,
        minio_container_endpoint,
        8081,
    )
    .await;
    let _lambda_8080 = start_lambda(
        &network,
        dynamodb_container_endpoint,
        minio_container_endpoint,
        8080,
    )
    .await;
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
    let (_azurite, azurite_host_connection_string, azurite_container_connection_string) =
        start_local_azurite(&network, 10000).await;

    // Start LocalStack for AWS provider in test modules
    let (_localstack, localstack_endpoint) = start_local_localstack(&network, 4566).await;
    env::set_var("AWS_ENDPOINT_URL", &localstack_endpoint);

    env::set_var(
        "AZURE_STORAGE_CONNECTION_STRING",
        &azurite_host_connection_string,
    );
    env::set_var("ARM_SKIP_PROVIDER_REGISTRATION", "true");
    env::set_var("AZURE_HTTP_USER_AGENT", "azurite-test");

    let _azure_8080: ContainerAsync<GenericImage> = start_azure_function(
        &network,
        "http://cosmos:8081",
        &azurite_container_connection_string,
        8080,
    )
    .await;
    let _azure_8081: ContainerAsync<GenericImage> = start_azure_function(
        &network,
        "http://cosmos:8081",
        &azurite_container_connection_string,
        8081,
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
    let base_dir = integration_tests_dir();
    let lambda_source = base_dir.join("lambda-code/test-api.py");
    let bootstrap_source = base_dir.join("lambda-code/bootstrap.py");

    let container_port = 8080;
    let container = MirroredImage::LambdaPython
        .generic_image()
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
    container
}

pub async fn start_azure_function(
    network: &str,
    cosmos_endpoint: &str,
    azurite_connection_string: &str,
    port: u16,
) -> ContainerAsync<GenericImage> {
    let base_dir = integration_tests_dir();
    let container_port = 80;

    let image = MirroredImage::AzureFunctionsPython.generic_image()
        .with_exposed_port(container_port.tcp())
        .with_wait_for(WaitFor::message_on_stdout("Application started. Press Ctrl+C to shut down."))
        .with_copy_to("/home/site/wwwroot", base_dir.join("azure-function-code"))
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
        .with_name(MirroredImage::DynamoDb.repository())
        .with_tag(MirroredImage::DynamoDb.tag())
        .with_network(network)
        .with_container_name("dynamodb")
        .with_mapped_port(port, 8000.tcp())
        .start()
        .await
        .unwrap();

    let dynamodb_host_port = db.get_host_port_ipv4(8000).await.unwrap();
    let dynamodb_endpoint = format!("http://127.0.0.1:{}", dynamodb_host_port);
    (db, dynamodb_endpoint)
}

pub async fn start_local_cosmosdb(network: &str, port: u16) -> ContainerAsync<GenericImage> {
    // Remove any stale container from a previous run (ignore errors if it doesn't exist)
    let _ = tokio::process::Command::new("docker")
        .args(["rm", "-f", "cosmos"])
        .output()
        .await;

    let container_port = 8081;

    let image = MirroredImage::CosmosEmulator
        .generic_image()
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
    let minio = MirroredImage::Minio
        .generic_image()
        .with_network(network)
        .with_container_name("minio")
        .with_env_var("MINIO_ACCESS_KEY", "minio")
        .with_env_var("MINIO_SECRET_KEY", "minio123")
        .with_cmd(["server", "/data"])
        .with_mapped_port(port, 9000.tcp())
        .start()
        .await
        .expect("Failed to start minio");

    let minio_host_port = minio.get_host_port_ipv4(9000).await.unwrap();
    let minio_host_endpoint = format!("http://127.0.0.1:{}", minio_host_port);

    (minio, minio_host_endpoint)
}

pub async fn start_local_azurite(
    network: &str,
    host_port: u16,
) -> (ContainerAsync<GenericImage>, String, String) {
    let azurite_blob_port = 10000.tcp();

    let image = MirroredImage::Azurite
        .generic_image()
        .with_exposed_port(azurite_blob_port)
        .with_wait_for(WaitFor::message_on_stdout(
            "Azurite Blob service is successfully listening at",
        ))
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

    let azurite_host_endpoint = format!("http://127.0.0.1:{}", actual_mapped_port);
    let azurite_host_connection_string = format!(
        "DefaultEndpointsProtocol=http;AccountName=devstoreaccount1;AccountKey=Eby8vdM02xNOcqFlqUwJPLlmEtlCDXJ1OUzFT50uSRZ6IFsuFq2UVErCz4I6tq/K1SZFPTOtr/KBHBeksoGMGw==;BlobEndpoint={};",
        azurite_host_endpoint
    );

    let azurite_container_endpoint = "http://azurite:10000";
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
    let random_id: u32 = rand::random();
    format!("testcontainers-network-{}", random_id)
}

pub async fn start_local_localstack(
    network: &str,
    port: u16,
) -> (ContainerAsync<LocalStack>, String) {
    let localstack = LocalStack::default()
        .with_name(MirroredImage::LocalStack.repository())
        .with_tag(MirroredImage::LocalStack.tag())
        .with_network(network)
        .with_mapped_port(port, 4566.tcp())
        .start()
        .await
        .unwrap();

    let localstack_host_port = localstack.get_host_port_ipv4(4566).await.unwrap();
    let localstack_endpoint = format!("http://127.0.0.1:{}", localstack_host_port);

    (localstack, localstack_endpoint)
}

/// Start a Jaeger all-in-one container, which accepts OTLP/HTTP on 4318 and
/// exposes a query API on 16686. Lets a test assert on the spans a service
/// actually exported, rather than trusting that export was configured.
///
/// Returns the container (keep it alive for the duration of the test), the OTLP
/// endpoint to point `OTEL_EXPORTER_OTLP_ENDPOINT` at, and the query base URL.
pub async fn start_jaeger() -> (ContainerAsync<GenericImage>, String, String) {
    let jaeger = MirroredImage::Jaeger
        .generic_image()
        .with_exposed_port(4318.tcp())
        .with_exposed_port(16686.tcp())
        .with_env_var("COLLECTOR_OTLP_ENABLED", "true")
        .start()
        .await
        .unwrap();

    let otlp_port = jaeger.get_host_port_ipv4(4318).await.unwrap();
    let query_port = jaeger.get_host_port_ipv4(16686).await.unwrap();

    (
        jaeger,
        format!("http://127.0.0.1:{}", otlp_port),
        format!("http://127.0.0.1:{}", query_port),
    )
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_mirrored_image_is_in_the_mirror_list() {
        let path = integration_tests_dir().join("../.github/vars/default.image_mirror.json");
        let content = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("Failed to read {}: {}", path.display(), e));
        let entries: Vec<serde_json::Value> =
            serde_json::from_str(&content).expect("Mirror list is not a JSON array");
        let mirrored: Vec<&str> = entries.iter().filter_map(|e| e["to"].as_str()).collect();

        let missing: Vec<String> = MirroredImage::ALL
            .iter()
            .map(|img| img.mirror_entry())
            .filter(|entry| !mirrored.contains(&entry.as_str()))
            .collect();
        assert!(
            missing.is_empty(),
            "Missing from {}: {:?}",
            path.display(),
            missing
        );
    }
}
