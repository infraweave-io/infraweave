use crate::reaper::{ensure_reaper, lock, run_id, state_dir, RUN_LABEL};
use env_common::interface::{initialize_project_id_and_region, GenericCloudHandler};
use env_defs::CloudProvider;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::env;
use std::future::Future;
use std::path::PathBuf;
use std::process::Command;
use std::sync::OnceLock;
use std::time::Duration;
use testcontainers::core::BuildImageOptions;
use testcontainers::core::{IntoContainerPort, Mount, WaitFor};
use testcontainers::runners::AsyncBuilder;
use testcontainers::ContainerAsync;
use testcontainers::{runners::AsyncRunner, GenericBuildableImage, GenericImage, ImageExt};

/// AWS emulator (DynamoDB, S3 and the services the Terraform AWS provider touches).
pub const FLOCI_IMAGE: &str = "floci/floci";
pub const FLOCI_TAG: &str = "2.5.0";
pub const FLOCI_PORT: u16 = 4566;
/// Azure emulator (Cosmos DB NoSQL and Blob storage).
pub const FLOCI_AZ_IMAGE: &str = "floci/floci-az";
pub const FLOCI_AZ_TAG: &str = "0.17.0";
pub const FLOCI_AZ_PORT: u16 = 4577;
/// Locally built Azure Functions image with the test function baked in.
pub const AZURE_FUNCTION_IMAGE: &str = "infraweave-test-azure-function";
/// Trace backend used by the telemetry integration test: accepts OTLP and
/// exposes a query API to assert on what actually arrived.
pub const JAEGER_IMAGE: &str = "jaegertracing/all-in-one";
pub const JAEGER_TAG: &str = "1.62.0";

pub const SHARED_TEST_NETWORK: &str = "infraweave-integration-tests";
/// Label on every container the integration tests start, for cleanup after a run.
pub const TEST_CONTAINER_LABEL: &str = "infraweave.integration-test";
/// Directory caching downloaded provider binaries across tests (optional).
pub const DOWNLOAD_CACHE_ENV: &str = "INFRAWEAVE_DOWNLOAD_CACHE_DIR";

const AZURE_STORAGE_ACCOUNT: &str = "devstoreaccount1";
const AZURE_STORAGE_KEY: &str =
    "Eby8vdM02xNOcqFlqUwJPLlmEtlCDXJ1OUzFT50uSRZ6IFsuFq2UVErCz4I6tq/K1SZFPTOtr/KBHBeksoGMGw==";
const COSMOS_KEY: &str =
    "C2y6yDjf5/R+ob0N8A7Cgv30VRDJIWEHLM+4QDU5DE2nQ9nDuVTqobD4b8mGGyPMbIZnqyMsEcaGQy67XIw/Jw==";

static SHARED_NETWORK_CHECK: OnceLock<()> = OnceLock::new();

/// Endpoints of a running emulator set, enough to configure a test process.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Endpoints {
    pub test_id: String,
    /// Host-reachable floci (AWS) endpoint, also used as the Terraform AWS provider target.
    pub aws_endpoint: String,
    /// Host-reachable floci-az endpoint (Azure only).
    pub azure_endpoint: Option<String>,
    /// Host-reachable Azure Functions URL serving the test API (Azure only).
    pub function_endpoint: Option<String>,
}

/// Emulator containers for one test (or one shared fixture) and their endpoints.
/// Dropping it removes the containers.
pub struct Emulators {
    pub containers: Vec<ContainerAsync<GenericImage>>,
    pub endpoints: Endpoints,
}

/// Handles to the emulators a test runs against.
#[derive(Clone)]
pub struct TestContext {
    pub test_id: String,
    pub api_endpoint: String,
    pub bootstrap_endpoint: String,
    pub api_handler: GenericCloudHandler,
    pub bootstrap_handler: GenericCloudHandler,
    /// Host-reachable floci (AWS) endpoint, also used as the Terraform AWS provider target.
    pub aws_endpoint: String,
}

impl TestContext {
    pub async fn from_endpoints(endpoints: &Endpoints) -> Self {
        // AWS direct mode ignores the function endpoint and talks to DynamoDB/S3 directly.
        let function_endpoint = endpoints
            .function_endpoint
            .clone()
            .unwrap_or_else(|| endpoints.aws_endpoint.clone());
        let handler = GenericCloudHandler::custom(&function_endpoint).await;

        Self {
            test_id: endpoints.test_id.clone(),
            api_endpoint: function_endpoint.clone(),
            bootstrap_endpoint: function_endpoint,
            api_handler: handler.clone(),
            bootstrap_handler: handler,
            aws_endpoint: endpoints.aws_endpoint.clone(),
        }
    }
}

/// Returns the path to the integration-tests directory (resolved at compile time).
pub fn integration_tests_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

pub fn get_image_name(original_image: &str, tag: &str) -> (String, String) {
    let registry_prefix = env::var("DOCKER_IMAGE_MIRROR").unwrap_or_default();

    if registry_prefix.is_empty() {
        return (original_image.to_string(), tag.to_string());
    }

    let mirrored_name = match original_image {
        "mcr.microsoft.com/azure-functions/python" => "azure-functions-python",
        JAEGER_IMAGE => "jaeger-all-in-one",
        _ => return (original_image.to_string(), tag.to_string()),
    };

    let full_image = format!("{}/{}", registry_prefix, mirrored_name);
    (full_image, tag.to_string())
}

pub fn is_azure() -> bool {
    env::var("PROVIDER").unwrap_or("azure".to_string()) == "azure"
}

/// Runs `function_to_test` against its own freshly started and bootstrapped emulators.
pub async fn test_scaffold<F, Fut>(function_to_test: F)
where
    F: FnOnce(TestContext) -> Fut,
    Fut: Future<Output = ()>,
{
    let emulators = start_emulators(&generate_random_test_id()).await;
    configure_env(&emulators.endpoints);
    initialize_project_id_and_region().await;

    let context = TestContext::from_endpoints(&emulators.endpoints).await;
    bootstrap_tables(&context).await;
    bootstrap_buckets(&context).await;

    function_to_test(context).await;
}

/// Starts the emulators for the current provider, labelled for this run's reaper.
pub async fn start_emulators(test_id: &str) -> Emulators {
    ensure_reaper().await;
    let network = shared_test_network();

    // floci serves the AWS control plane (DynamoDB + S3, incl. Terraform state)
    // and, for both providers, the Terraform AWS provider used by the test modules.
    let (floci, aws_endpoint) = start_floci(network, test_id).await;
    println!("floci started at: {}", aws_endpoint);

    if !is_azure() {
        return Emulators {
            containers: vec![floci],
            endpoints: Endpoints {
                test_id: test_id.to_string(),
                aws_endpoint,
                azure_endpoint: None,
                function_endpoint: None,
            },
        };
    }

    let (floci_az, floci_az_host_endpoint, floci_az_container_endpoint) =
        start_floci_az(network, test_id).await;
    println!("floci-az started at: {}", floci_az_host_endpoint);

    let azure_function = AzureFunctionConfig {
        cosmos_endpoint: format!(
            "{}/{}-cosmos",
            floci_az_container_endpoint, AZURE_STORAGE_ACCOUNT
        ),
        blob_connection_string: blob_connection_string(&floci_az_container_endpoint),
        blob_host_endpoint: floci_az_host_endpoint.clone(),
    };
    // One Functions host serves both the API and bootstrap calls; they run the
    // same code with the same configuration.
    let (azure_function, function_endpoint) = start_azure_function(network, &azure_function).await;

    Emulators {
        containers: vec![floci, floci_az, azure_function],
        endpoints: Endpoints {
            test_id: test_id.to_string(),
            aws_endpoint,
            azure_endpoint: Some(floci_az_host_endpoint),
            function_endpoint: Some(function_endpoint),
        },
    }
}

/// Points this process (SDK clients, Terraform) at the given emulators.
pub fn configure_env(endpoints: &Endpoints) {
    env::set_var("AWS_ENDPOINT_URL", &endpoints.aws_endpoint);
    env::set_var("AWS_ACCESS_KEY_ID", "test");
    env::set_var("AWS_SECRET_ACCESS_KEY", "test");
    // Region must be set since its not set in AWS tf-module
    env::set_var("AWS_REGION", "us-west-2");

    let Some(azure_endpoint) = &endpoints.azure_endpoint else {
        env::set_var("MINIO_ENDPOINT", &endpoints.aws_endpoint);
        env::set_var("DYNAMODB_ENDPOINT", &endpoints.aws_endpoint);
        return;
    };

    env::set_var(
        "AZURE_STORAGE_CONNECTION_STRING",
        blob_connection_string(azure_endpoint),
    );
    env::set_var("ARM_SKIP_PROVIDER_REGISTRATION", "true");
    env::set_var("AZURE_HTTP_USER_AGENT", "floci-az-test");

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
}

pub fn generate_random_test_id() -> String {
    let random_id: u32 = rand::random();
    format!("test-{}", random_id)
}

/// Docker network shared by all tests; containers are told apart by name.
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

        // Another test process may have created it in the meantime.
        let create_stderr = String::from_utf8_lossy(&create_output.stderr);
        if !create_output.status.success() && !create_stderr.contains("already exists") {
            panic!(
                "Failed to ensure Docker network '{SHARED_TEST_NETWORK}' exists: {}",
                create_stderr.trim()
            );
        }
    });

    SHARED_TEST_NETWORK
}

pub fn service_container_name(test_id: &str, service: &str) -> String {
    format!("{service}-{test_id}")
}

/// Labels for every test container; the run label is what the reaper removes by.
pub fn container_labels() -> [(&'static str, &'static str); 2] {
    [(TEST_CONTAINER_LABEL, "true"), (RUN_LABEL, run_id())]
}

fn blob_connection_string(endpoint: &str) -> String {
    format!(
        "DefaultEndpointsProtocol=http;AccountName={account};AccountKey={key};BlobEndpoint={endpoint}/{account};",
        account = AZURE_STORAGE_ACCOUNT,
        key = AZURE_STORAGE_KEY,
    )
}

/// Starts floci and returns the container with its host-reachable endpoint.
pub async fn start_floci(network: &str, test_id: &str) -> (ContainerAsync<GenericImage>, String) {
    let container = GenericImage::new(FLOCI_IMAGE, FLOCI_TAG)
        .with_exposed_port(FLOCI_PORT.tcp())
        .with_wait_for(WaitFor::message_on_stdout(format!(
            "Listening on: http://0.0.0.0:{}",
            FLOCI_PORT
        )))
        .with_network(network)
        .with_container_name(service_container_name(test_id, "floci"))
        .with_labels(container_labels())
        .with_env_var("FLOCI_DEFAULT_REGION", "us-west-2")
        .with_env_var("FLOCI_STORAGE_MODE", "memory")
        .with_env_var("FLOCI_SERVICES_UI_ENABLED", "false")
        .start()
        .await
        .expect("Failed to start floci");

    let port = container.get_host_port_ipv4(FLOCI_PORT).await.unwrap();
    (container, format!("http://127.0.0.1:{}", port))
}

/// Starts floci-az and returns the container with its host-reachable and
/// network-internal endpoints.
pub async fn start_floci_az(
    network: &str,
    test_id: &str,
) -> (ContainerAsync<GenericImage>, String, String) {
    let container_name = service_container_name(test_id, "floci-az");
    let container = GenericImage::new(FLOCI_AZ_IMAGE, FLOCI_AZ_TAG)
        .with_exposed_port(FLOCI_AZ_PORT.tcp())
        .with_wait_for(WaitFor::message_on_stdout("floci-az is ready"))
        .with_network(network)
        .with_container_name(&container_name)
        .with_labels(container_labels())
        .with_env_var("FLOCI_AZ_STORAGE_MODE", "memory")
        .with_env_var("FLOCI_AZ_SERVICES_COSMOS_ENGINES_NOSQL_ENABLED", "true")
        .start()
        .await
        .expect("Failed to start floci-az");

    let port = container.get_host_port_ipv4(FLOCI_AZ_PORT).await.unwrap();
    (
        container,
        format!("http://127.0.0.1:{}", port),
        format!("http://{}:{}", container_name, FLOCI_AZ_PORT),
    )
}

pub struct AzureFunctionConfig {
    /// Cosmos DB endpoint reachable from inside the Docker network.
    pub cosmos_endpoint: String,
    /// Blob connection string reachable from inside the Docker network.
    pub blob_connection_string: String,
    /// Blob endpoint reachable from the host, used for SAS URLs handed to tests.
    pub blob_host_endpoint: String,
}

/// Builds the Azure Functions image with the test function and its Python
/// dependencies baked in, once per change to the function code. Installing
/// them at container start costs every test a slow `pip install`, which runs
/// under emulation on arm64 hosts.
async fn azure_function_image() -> GenericImage {
    let function_dir = integration_tests_dir().join("azure-function-code");
    let (base_name, base_tag) = get_image_name("mcr.microsoft.com/azure-functions/python", "4.0");
    let base_image = format!("{base_name}:{base_tag}");

    let mut hasher = Sha256::new();
    hasher.update(base_image.as_bytes());
    let mut files: Vec<_> = std::fs::read_dir(&function_dir)
        .expect("Failed to read azure-function-code")
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.is_file())
        .collect();
    files.sort();
    for file in &files {
        hasher.update(file.file_name().unwrap().as_encoded_bytes());
        hasher.update(std::fs::read(file).unwrap());
    }
    let tag = format!("{:x}", hasher.finalize())[..16].to_string();

    // One build at a time; the rest find the image and skip.
    let _lock = lock(&state_dir(), "azure-function-image").await;
    GenericBuildableImage::new(AZURE_FUNCTION_IMAGE, tag)
        .with_dockerfile_string(format!(
            "FROM {base_image}\n\
             COPY app/requirements.txt /home/site/wwwroot/requirements.txt\n\
             RUN pip install --no-cache-dir -r /home/site/wwwroot/requirements.txt\n\
             COPY app /home/site/wwwroot\n"
        ))
        .with_file(function_dir, "./app")
        .build_image_with(BuildImageOptions::new().with_skip_if_exists(true))
        .await
        .expect("Failed to build the Azure Functions image")
}

/// Starts the Python Azure Functions host serving the test API and returns
/// the container with its host-reachable URL.
pub async fn start_azure_function(
    network: &str,
    config: &AzureFunctionConfig,
) -> (ContainerAsync<GenericImage>, String) {
    let container_port = 80;

    let mut request = azure_function_image()
        .await
        .with_exposed_port(container_port.tcp())
        .with_wait_for(WaitFor::message_on_stdout(
            "Application started. Press Ctrl+C to shut down.",
        ))
        // Slow under amd64 emulation on arm64 hosts.
        .with_startup_timeout(Duration::from_secs(120))
        .with_env_var("AzureWebJobsStorage", "UseDevelopmentStorage=true")
        .with_env_var("AzureWebJobsScriptRoot", "/home/site/wwwroot")
        .with_env_var("FUNCTIONS_WORKER_RUNTIME", "python")
        .with_env_var("COSMOS_DB_ENDPOINT", &config.cosmos_endpoint)
        .with_env_var("COSMOS_DB_DATABASE", "iw_database")
        .with_env_var("COSMOS_KEY", COSMOS_KEY)
        .with_env_var("BLOB_CONNECTION_STRING", &config.blob_connection_string)
        .with_env_var("BLOB_HOST_ENDPOINT", &config.blob_host_endpoint)
        .with_network(network)
        .with_labels(container_labels());
    // Share provider binary downloads with the host and other tests.
    if let Ok(cache_dir) = env::var(DOWNLOAD_CACHE_ENV) {
        request = request
            .with_mount(Mount::bind_mount(cache_dir, "/download-cache"))
            .with_env_var(DOWNLOAD_CACHE_ENV, "/download-cache");
    }
    let container = request
        .start()
        .await
        .expect("Failed to start Azure Functions container");

    let port = container.get_host_port_ipv4(container_port).await.unwrap();
    (container, format!("http://127.0.0.1:{}", port))
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

/// Start a Jaeger all-in-one container, which accepts OTLP/HTTP on 4318 and
/// exposes a query API on 16686. Lets a test assert on the spans a service
/// actually exported, rather than trusting that export was configured.
///
/// Returns the container (keep it alive for the duration of the test), the OTLP
/// endpoint to point `OTEL_EXPORTER_OTLP_ENDPOINT` at, and the query base URL.
pub async fn start_jaeger() -> (ContainerAsync<GenericImage>, String, String) {
    ensure_reaper().await;
    let (image_name, image_tag) = get_image_name(JAEGER_IMAGE, JAEGER_TAG);
    let jaeger = GenericImage::new(&image_name, &image_tag)
        .with_exposed_port(4318.tcp())
        .with_exposed_port(16686.tcp())
        .with_labels(container_labels())
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
