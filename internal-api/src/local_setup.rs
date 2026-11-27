#![cfg(feature = "local")]
use env_common::interface::GenericCloudHandler;
use env_common::logic::{publish_module, publish_provider};
use testcontainers::core::{IntoContainerPort, WaitFor};
use testcontainers::{runners::AsyncRunner, ContainerAsync, GenericImage, ImageExt};

/// Local AWS emulator serving DynamoDB and S3 from one endpoint.
pub const FLOCI_IMAGE: &str = "floci/floci";
pub const FLOCI_TAG: &str = "2.5.0";
pub const FLOCI_PORT: u16 = 4566;

pub struct LocalInfra {
    pub floci: ContainerAsync<GenericImage>,
}

/// Whether to start the embedded floci container.
///
/// When `false`, the server skips local infrastructure and talks to whatever
/// cloud resources the process environment points at (e.g. a sourced `.env`
/// with real AWS credentials). Auth verification remains bypassed because the
/// `local` feature is compiled in regardless. Controlled by `LOCAL_INFRA`
/// (`1`/`true`/`yes`/`on`).
pub fn local_infra_enabled() -> bool {
    match std::env::var("LOCAL_INFRA") {
        Ok(v) => matches!(
            v.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        ),
        Err(_) => false,
    }
}

pub async fn start_local_infrastructure() -> anyhow::Result<LocalInfra> {
    println!("Starting local infrastructure...");

    let floci_container = GenericImage::new(FLOCI_IMAGE, FLOCI_TAG)
        .with_exposed_port(FLOCI_PORT.tcp())
        .with_wait_for(WaitFor::message_on_stdout(format!(
            "Listening on: http://0.0.0.0:{}",
            FLOCI_PORT
        )))
        .with_env_var("FLOCI_STORAGE_MODE", "memory")
        .with_env_var("FLOCI_SERVICES_UI_ENABLED", "false")
        .start()
        .await
        .map_err(|e| anyhow::anyhow!("Failed to start floci: {}", e))?;

    let floci_port = floci_container.get_host_port_ipv4(FLOCI_PORT).await?;
    // Force use of localhost for Colima compatibility (avoids issues with non-localhost container hosts)
    let floci_endpoint = format!("http://localhost:{}/", floci_port);
    println!("floci started at {}", floci_endpoint);

    // Set env vars so standard AWS SDK usage picks them up
    std::env::set_var("DYNAMODB_ENDPOINT", &floci_endpoint);
    std::env::set_var("DYNAMODB_ENDPOINT_URL", &floci_endpoint);
    std::env::set_var("AWS_ENDPOINT_URL_DYNAMODB", &floci_endpoint);
    std::env::set_var("MINIO_ENDPOINT", &floci_endpoint);
    std::env::set_var("AWS_ENDPOINT_URL_S3", &floci_endpoint);
    std::env::set_var("AWS_S3_FORCE_PATH_STYLE", "true");

    // Configure AWS auth for local; floci accepts any credentials
    std::env::set_var("AWS_ACCESS_KEY_ID", "test");
    std::env::set_var("AWS_SECRET_ACCESS_KEY", "test");

    // Set default business logic env vars if they are missing
    if std::env::var("CLOUD_PROVIDER").is_err() {
        std::env::set_var("CLOUD_PROVIDER", "aws");
    }
    if std::env::var("AWS_REGION").is_err() {
        std::env::set_var("AWS_REGION", "us-west-2");
    }
    if std::env::var("ENVIRONMENT").is_err() {
        std::env::set_var("ENVIRONMENT", "dev");
    }
    if std::env::var("INFRAWEAVE_ENVIRONMENT").is_err() {
        std::env::set_var("INFRAWEAVE_ENVIRONMENT", "dev");
    }
    if std::env::var("INFRAWEAVE_ENV").is_err() {
        std::env::set_var("INFRAWEAVE_ENV", "dev");
    }
    if std::env::var("ACCOUNT_ID").is_err() {
        std::env::set_var("ACCOUNT_ID", "000000000000");
    }

    // Enable docker runner by default for local dev
    if std::env::var("INFRAWEAVE_DOCKER_RUNNER").is_err() {
        println!("Setting INFRAWEAVE_DOCKER_RUNNER to infraweave/runner:latest");
        std::env::set_var("INFRAWEAVE_DOCKER_RUNNER", "infraweave/runner:latest");
    }
    if std::env::var("CENTRAL_ACCOUNT_ID").is_err() {
        std::env::set_var("CENTRAL_ACCOUNT_ID", "000000000000");
    }
    if std::env::var("NOTIFICATION_TOPIC_ARN").is_err() {
        let region = std::env::var("AWS_REGION").unwrap_or_else(|_| "us-west-2".to_string());
        std::env::set_var(
            "NOTIFICATION_TOPIC_ARN",
            format!(
                "arn:aws:sns:{}:000000000000:infraweave-notifications",
                region
            ),
        );
    }

    // Bootstrap tables/buckets
    bootstrap_resources().await?;

    Ok(LocalInfra {
        floci: floci_container,
    })
}

async fn bootstrap_resources() -> anyhow::Result<()> {
    let dynamodb_endpoint = std::env::var("DYNAMODB_ENDPOINT")?;
    let s3_endpoint = std::env::var("AWS_ENDPOINT_URL_S3")?;
    let region = std::env::var("AWS_REGION").unwrap_or_else(|_| "us-west-2".to_string());

    // Create DynamoDB tables and seed data
    env_aws_direct::bootstrap_dynamodb_tables(&dynamodb_endpoint, &region).await?;

    // Create S3 buckets
    env_aws_direct::create_s3_buckets(&s3_endpoint, &region).await?;

    // Publish a sample module from integration tests
    let handler = GenericCloudHandler::default().await;

    // Publish fake provider (aws-5) first
    let provider_path = "integration-tests/providers/aws-5";
    if std::path::Path::new(provider_path).exists() {
        println!("Publishing sample provider: {}", provider_path);
        match publish_provider(&handler, provider_path, Some("0.1.2")).await {
            Ok(_) => println!("Provider aws published successfully!"),
            Err(e) => println!("Warning: Failed to publish provider: {:?}", e),
        }
    } else {
        println!("Warning: Provider path '{}' not found.", provider_path);
    }

    // Assume running from workspace root
    let module_path = "integration-tests/modules/s3bucket-simple";
    if std::path::Path::new(module_path).exists() {
        println!("Publishing sample module: {}", module_path);
        // Using "stable" track and version "1.0.0"
        match publish_module(&handler, module_path, "stable", Some("1.0.0"), None).await {
            Ok(_) => println!("Module s3bucket-simple published successfully!"),
            Err(e) => println!("Warning: Failed to publish module: {:?}", e),
        }
    } else {
        println!(
            "Warning: Module path '{}' not found. Cannot publish sample module.",
            module_path
        );
    }

    println!("\nLocal infrastructure ready!");
    println!("To use the CLI with local mode:");
    println!("  cargo run --bin cli --features local -- module list dev");

    Ok(())
}

/// Start the HTTP server on a random port and return that port.
/// Runs for the lifetime of the process; no shutdown is provided.
pub fn start_test_server() -> u16 {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new().expect("Failed to create server runtime");
        rt.block_on(async {
            let app = crate::http_router::create_router();
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
                .await
                .expect("Failed to bind test server");
            let port = listener.local_addr().unwrap().port();
            tx.send(port).unwrap();
            axum::serve(listener, app).await.unwrap();
        });
    });
    rx.recv()
        .expect("Failed to receive port from server thread")
}
