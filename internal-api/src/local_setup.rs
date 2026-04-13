#![cfg(feature = "local")]
use env_common::interface::GenericCloudHandler;
use env_common::logic::{publish_module, publish_provider};
use testcontainers::{runners::AsyncRunner, ContainerAsync, GenericImage, ImageExt};
use testcontainers_modules::dynamodb_local::DynamoDb;

pub struct LocalInfra {
    pub dynamodb: ContainerAsync<DynamoDb>,
    pub minio: ContainerAsync<GenericImage>,
}

pub async fn start_local_infrastructure() -> anyhow::Result<LocalInfra> {
    println!("Starting local infrastructure...");

    // Start DynamoDB - attempt to use port 8000, but verify what we actually got
    let dynamo_container = DynamoDb::default()
        .with_mapped_port(8000, 8000.into())
        .start()
        .await
        .map_err(|e| anyhow::anyhow!("Failed to start DynamoDB: {}", e))?;

    let dynamo_port = dynamo_container.get_host_port_ipv4(8000).await?;
    // Force use of localhost for Colima compatibility (avoids issues with non-localhost container hosts)
    let dynamo_endpoint = format!("http://localhost:{}/", dynamo_port);
    println!("DynamoDB started at {}", dynamo_endpoint);
    println!("NOTE: CLI should use port {} for DynamoDB", dynamo_port);

    // Set env vars so standard AWS SDK usage picks them up
    std::env::set_var("DYNAMODB_ENDPOINT", &dynamo_endpoint);
    std::env::set_var("DYNAMODB_ENDPOINT_URL", &dynamo_endpoint);
    std::env::set_var("AWS_ENDPOINT_URL_DYNAMODB", &dynamo_endpoint);

    // Start MinIO - attempt to use port 9000, but verify what we actually got
    let minio_container = GenericImage::new("minio/minio", "latest")
        .with_mapped_port(9000, 9000.into())
        .with_env_var("MINIO_ACCESS_KEY", "minio")
        .with_env_var("MINIO_SECRET_KEY", "minio123")
        .with_env_var("MINIO_ROOT_USER", "minio")
        .with_env_var("MINIO_ROOT_PASSWORD", "minio123")
        .with_cmd(vec!["server", "/data"])
        .start()
        .await
        .map_err(|e| anyhow::anyhow!("Failed to start MinIO: {}", e))?;

    let minio_host = minio_container.get_host().await?;
    let minio_port = minio_container.get_host_port_ipv4(9000).await?;
    let minio_endpoint = format!("http://{}:{}/", minio_host, minio_port);
    println!("MinIO started at {}", minio_endpoint);
    println!("NOTE: CLI should use port {} for S3", minio_port);

    // Wait for MinIO to be ready
    let mut minio_ready = false;
    for i in 0..10 {
        let resp = reqwest::get(format!("{}minio/health/live", minio_endpoint)).await;
        if resp.is_ok() {
            minio_ready = true;
            println!("MinIO is ready.");
            break;
        }
        println!("Waiting for MinIO... ({}/10)", i + 1);
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    }
    if !minio_ready {
        println!("Warning: MinIO health check failed, proceeding anyway but errors may occur.");
    }

    std::env::set_var("MINIO_ENDPOINT", &minio_endpoint);
    std::env::set_var("AWS_ENDPOINT_URL_S3", &minio_endpoint);
    std::env::set_var("AWS_S3_FORCE_PATH_STYLE", "true"); // Important for local MinIO

    // Configure AWS auth/region for local
    std::env::set_var("AWS_ACCESS_KEY_ID", "minio");
    std::env::set_var("AWS_SECRET_ACCESS_KEY", "minio123");

    // Set default business logic env vars if they are missing
    if std::env::var("CLOUD_PROVIDER").is_err() {
        std::env::set_var("CLOUD_PROVIDER", "aws_direct");
    }
    if std::env::var("AWS_REGION").is_err() {
        std::env::set_var("AWS_REGION", "us-west-2");
    }
    // Enable TEST_MODE so env_aws_direct uses local DynamoDB/MinIO endpoints
    // and is_http_mode_enabled() returns false (avoids stale ~/.infraweave/tokens.json)
    if std::env::var("TEST_MODE").is_err() {
        std::env::set_var("TEST_MODE", "true");
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
        dynamodb: dynamo_container,
        minio: minio_container,
    })
}

async fn bootstrap_resources() -> anyhow::Result<()> {
    let dynamodb_endpoint = std::env::var("DYNAMODB_ENDPOINT")?;
    let minio_endpoint = std::env::var("MINIO_ENDPOINT")?;
    let region = std::env::var("AWS_REGION").unwrap_or_else(|_| "us-west-2".to_string());

    // Create DynamoDB tables and seed data
    env_aws_direct::bootstrap_dynamodb_tables(&dynamodb_endpoint, &region).await?;

    // Create S3 buckets
    env_aws_direct::create_s3_buckets(&minio_endpoint, &region).await?;

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
