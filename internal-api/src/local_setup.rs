#![cfg(feature = "local")]
use aws_config::BehaviorVersion;
use aws_sdk_dynamodb::types::{
    AttributeDefinition, BillingMode, GlobalSecondaryIndex, KeySchemaElement, KeyType, Projection,
    ProjectionType, ScalarAttributeType,
};
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
        std::env::set_var("CLOUD_PROVIDER", "aws");
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
    // Get the DynamoDB endpoint we set earlier
    let dynamodb_endpoint = std::env::var("DYNAMODB_ENDPOINT")?;

    let region = std::env::var("AWS_REGION").unwrap_or_else(|_| "us-west-2".to_string());
    // Create DynamoDB client with explicit endpoint configuration
    let dynamodb_creds =
        aws_sdk_dynamodb::config::Credentials::new("minio", "minio123", None, None, "static");
    let dynamodb_config = aws_sdk_dynamodb::Config::builder()
        .behavior_version(BehaviorVersion::latest())
        .endpoint_url(&dynamodb_endpoint)
        .region(aws_sdk_dynamodb::config::Region::new(region.clone()))
        .credentials_provider(dynamodb_creds)
        .build();
    let dynamodb = aws_sdk_dynamodb::Client::from_conf(dynamodb_config);

    // For S3 (MinIO), we must be very explicit to avoid InvalidClientTokenId errors
    // which can happen if defaults pick up wrong credentials or region.
    let minio_endpoint = std::env::var("MINIO_ENDPOINT")?;
    let s3_creds = aws_sdk_s3::config::Credentials::new("minio", "minio123", None, None, "static");
    let s3_config = aws_sdk_s3::Config::builder()
        .behavior_version(BehaviorVersion::latest())
        .endpoint_url(&minio_endpoint)
        .region(aws_sdk_s3::config::Region::new(region.clone()))
        .credentials_provider(s3_creds)
        .force_path_style(true)
        .build();
    let s3 = aws_sdk_s3::Client::from_conf(s3_config);

    // Use centralized table name configuration from env_aws as single source of truth
    let table_map: std::collections::HashMap<&str, &str> =
        env_aws::DEFAULT_TABLE_NAMES.iter().copied().collect();

    for (env_var, table_name) in &table_map {
        // Always set env vars to match the tables we're creating, so the app points at the right table names.
        std::env::set_var(env_var, table_name);
        println!("Creating DynamoDB table: {}", table_name);

        let mut create_table = dynamodb.create_table().table_name(table_name.to_string());

        // Different tables have different schemas
        if *table_name == "permissions" {
            // Permissions table uses user_id as primary key
            create_table = create_table
                .attribute_definitions(
                    AttributeDefinition::builder()
                        .attribute_name("user_id")
                        .attribute_type(ScalarAttributeType::S)
                        .build()?,
                )
                .key_schema(
                    KeySchemaElement::builder()
                        .attribute_name("user_id")
                        .key_type(KeyType::Hash)
                        .build()?,
                )
                .billing_mode(BillingMode::PayPerRequest);
        } else {
            // Most tables use PK/SK pattern, including config table
            create_table = create_table
                .attribute_definitions(
                    AttributeDefinition::builder()
                        .attribute_name("PK")
                        .attribute_type(ScalarAttributeType::S)
                        .build()?,
                )
                .attribute_definitions(
                    AttributeDefinition::builder()
                        .attribute_name("SK")
                        .attribute_type(ScalarAttributeType::S)
                        .build()?,
                )
                .key_schema(
                    KeySchemaElement::builder()
                        .attribute_name("PK")
                        .key_type(KeyType::Hash)
                        .build()?,
                )
                .key_schema(
                    KeySchemaElement::builder()
                        .attribute_name("SK")
                        .key_type(KeyType::Range)
                        .build()?,
                )
                .billing_mode(BillingMode::PayPerRequest);

            if *table_name == "events" {
                create_table = create_table
                    .attribute_definitions(
                        AttributeDefinition::builder()
                            .attribute_name("PK_base_region")
                            .attribute_type(ScalarAttributeType::S)
                            .build()?,
                    )
                    .global_secondary_indexes(
                        GlobalSecondaryIndex::builder()
                            .index_name("RegionIndex")
                            .key_schema(
                                KeySchemaElement::builder()
                                    .attribute_name("PK_base_region")
                                    .key_type(KeyType::Hash)
                                    .build()?,
                            )
                            .key_schema(
                                KeySchemaElement::builder()
                                    .attribute_name("SK")
                                    .key_type(KeyType::Range)
                                    .build()?,
                            )
                            .projection(
                                Projection::builder()
                                    .projection_type(ProjectionType::All)
                                    .build(),
                            )
                            .build()?,
                    );
            } else if *table_name == "deployments" {
                create_table = create_table
                    .attribute_definitions(
                        AttributeDefinition::builder()
                            .attribute_name("deleted_PK")
                            .attribute_type(ScalarAttributeType::S)
                            .build()?,
                    )
                    .attribute_definitions(
                        AttributeDefinition::builder()
                            .attribute_name("deleted_PK_base")
                            .attribute_type(ScalarAttributeType::S)
                            .build()?,
                    )
                    .attribute_definitions(
                        AttributeDefinition::builder()
                            .attribute_name("module")
                            .attribute_type(ScalarAttributeType::S)
                            .build()?,
                    )
                    .attribute_definitions(
                        AttributeDefinition::builder()
                            .attribute_name("module_PK_base")
                            .attribute_type(ScalarAttributeType::S)
                            .build()?,
                    )
                    .attribute_definitions(
                        AttributeDefinition::builder()
                            .attribute_name("deleted_SK_base")
                            .attribute_type(ScalarAttributeType::S)
                            .build()?,
                    )
                    .attribute_definitions(
                        AttributeDefinition::builder()
                            .attribute_name("next_drift_check_epoch")
                            .attribute_type(ScalarAttributeType::N)
                            .build()?,
                    )
                    .global_secondary_indexes(
                        GlobalSecondaryIndex::builder()
                            .index_name("DeletedIndex")
                            .key_schema(
                                KeySchemaElement::builder()
                                    .attribute_name("deleted_PK_base")
                                    .key_type(KeyType::Hash)
                                    .build()?,
                            )
                            .key_schema(
                                KeySchemaElement::builder()
                                    .attribute_name("PK")
                                    .key_type(KeyType::Range)
                                    .build()?,
                            )
                            .projection(
                                Projection::builder()
                                    .projection_type(ProjectionType::All)
                                    .build(),
                            )
                            .build()?,
                    )
                    .global_secondary_indexes(
                        GlobalSecondaryIndex::builder()
                            .index_name("ModuleIndex")
                            .key_schema(
                                KeySchemaElement::builder()
                                    .attribute_name("module_PK_base")
                                    .key_type(KeyType::Hash)
                                    .build()?,
                            )
                            .key_schema(
                                KeySchemaElement::builder()
                                    .attribute_name("deleted_PK")
                                    .key_type(KeyType::Range)
                                    .build()?,
                            )
                            .projection(
                                Projection::builder()
                                    .projection_type(ProjectionType::All)
                                    .build(),
                            )
                            .build()?,
                    )
                    .global_secondary_indexes(
                        GlobalSecondaryIndex::builder()
                            .index_name("GlobalModuleIndex")
                            .key_schema(
                                KeySchemaElement::builder()
                                    .attribute_name("module")
                                    .key_type(KeyType::Hash)
                                    .build()?,
                            )
                            .key_schema(
                                KeySchemaElement::builder()
                                    .attribute_name("deleted_PK")
                                    .key_type(KeyType::Range)
                                    .build()?,
                            )
                            .projection(
                                Projection::builder()
                                    .projection_type(ProjectionType::All)
                                    .build(),
                            )
                            .build()?,
                    )
                    .global_secondary_indexes(
                        GlobalSecondaryIndex::builder()
                            .index_name("DriftCheckIndex")
                            .key_schema(
                                KeySchemaElement::builder()
                                    .attribute_name("deleted_SK_base")
                                    .key_type(KeyType::Hash)
                                    .build()?,
                            )
                            .key_schema(
                                KeySchemaElement::builder()
                                    .attribute_name("next_drift_check_epoch")
                                    .key_type(KeyType::Range)
                                    .build()?,
                            )
                            .projection(
                                Projection::builder()
                                    .projection_type(ProjectionType::All)
                                    .build(),
                            )
                            .build()?,
                    )
                    .global_secondary_indexes(
                        GlobalSecondaryIndex::builder()
                            .index_name("ReverseIndex") // Add missing ReverseIndex
                            .key_schema(
                                KeySchemaElement::builder()
                                    .attribute_name("SK")
                                    .key_type(KeyType::Hash)
                                    .build()?,
                            )
                            .key_schema(
                                KeySchemaElement::builder()
                                    .attribute_name("PK")
                                    .key_type(KeyType::Range)
                                    .build()?,
                            )
                            .projection(
                                Projection::builder()
                                    .projection_type(ProjectionType::All)
                                    .build(),
                            )
                            .build()?,
                    );
            }
        } // End of else block for non-permissions tables

        match create_table.send().await {
            Ok(_) => println!("Table {} created", table_name),
            Err(e) => {
                // Ignore ResourceInUseException (table exists)
                let err_str = e.to_string();
                if !err_str.contains("ResourceInUseException") {
                    println!("Error creating table {}: {}", table_name, e);
                    // Allow continuing?
                }
            }
        }
    }

    // Seed Config Table
    // bootstrap.py puts: PK="all_regions", data={"regions": ["us-west-2"]}
    // With SK support now
    let config_table_name = table_map.get("DYNAMODB_CONFIG_TABLE_NAME").unwrap();
    println!("Seeding Config Table: {}", config_table_name);

    let regions_val = aws_sdk_dynamodb::types::AttributeValue::L(vec![
        aws_sdk_dynamodb::types::AttributeValue::S(region.clone()),
    ]);
    let data_map = aws_sdk_dynamodb::types::AttributeValue::M(std::collections::HashMap::from([(
        "regions".to_string(),
        regions_val,
    )]));

    match dynamodb
        .put_item()
        .table_name(config_table_name.to_string())
        .item(
            "PK",
            aws_sdk_dynamodb::types::AttributeValue::S("all_regions".to_string()),
        )
        .item(
            "SK",
            aws_sdk_dynamodb::types::AttributeValue::S("config".to_string()),
        )
        .item("data", data_map)
        .send()
        .await
    {
        Ok(_) => println!("Seeded Config for all_regions"),
        Err(e) => println!("Error seeding config: {}", e),
    }

    // Seed sample projects to config table (now with SK)
    let config_table_name = table_map.get("DYNAMODB_CONFIG_TABLE_NAME").unwrap();
    println!("Seeding sample projects in table: {}", config_table_name);

    let sample_projects = vec![
        (
            "project-alpha",
            "Alpha Project",
            "Development project for testing",
        ),
        (
            "project-beta",
            "Beta Project",
            "Staging environment project",
        ),
        ("project-gamma", "Gamma Project", "Production workloads"),
    ];

    for (project_id, name, description) in sample_projects {
        let regions_list = vec![aws_sdk_dynamodb::types::AttributeValue::S(region.clone())];

        match dynamodb
            .put_item()
            .table_name(config_table_name.to_string())
            .item(
                "PK",
                aws_sdk_dynamodb::types::AttributeValue::S("PROJECTS".to_string()),
            )
            .item(
                "SK",
                aws_sdk_dynamodb::types::AttributeValue::S(format!("PROJECT#{}", project_id)),
            )
            .item(
                "project_id",
                aws_sdk_dynamodb::types::AttributeValue::S(project_id.to_string()),
            )
            .item(
                "name",
                aws_sdk_dynamodb::types::AttributeValue::S(name.to_string()),
            )
            .item(
                "description",
                aws_sdk_dynamodb::types::AttributeValue::S(description.to_string()),
            )
            .item(
                "regions",
                aws_sdk_dynamodb::types::AttributeValue::L(regions_list),
            )
            .item(
                "repositories",
                aws_sdk_dynamodb::types::AttributeValue::L(vec![]),
            )
            .send()
            .await
        {
            Ok(_) => println!("Seeded project: {}", project_id),
            Err(e) => println!("Error seeding project {}: {}", project_id, e),
        }
    }

    // Seed permissions table - grant local-user access to 2 out of 3 projects
    let permissions_table_name = table_map.get("DYNAMODB_PERMISSIONS_TABLE_NAME").unwrap();
    println!("Seeding permissions table: {}", permissions_table_name);

    let allowed_projects = vec![
        aws_sdk_dynamodb::types::AttributeValue::S("project-alpha".to_string()),
        aws_sdk_dynamodb::types::AttributeValue::S("project-beta".to_string()),
        // project-gamma is intentionally omitted to test access control
    ];

    match dynamodb
        .put_item()
        .table_name(permissions_table_name.to_string())
        .item(
            "user_id",
            aws_sdk_dynamodb::types::AttributeValue::S("local-user".to_string()),
        )
        .item(
            "allowed_projects",
            aws_sdk_dynamodb::types::AttributeValue::L(allowed_projects),
        )
        .send()
        .await
    {
        Ok(_) => {
            println!("Seeded permissions for local-user (access to project-alpha and project-beta)")
        }
        Err(e) => println!("Error seeding permissions: {}", e),
    }

    // Buckets - use centralized configuration from env_aws
    let bucket_map: std::collections::HashMap<&str, &str> =
        env_aws::DEFAULT_BUCKET_NAMES.iter().copied().collect();

    for (env_var, bucket_name) in &bucket_map {
        std::env::set_var(env_var, bucket_name);
        println!("Creating S3 bucket: {}", bucket_name);

        match s3
            .create_bucket()
            .bucket(bucket_name.to_string())
            .send()
            .await
        {
            Ok(_) => println!("Bucket {} created", bucket_name),
            Err(e) => {
                let err_str = e.to_string();
                if !err_str.contains("BucketAlreadyOwnedByYou")
                    && !err_str.contains("BucketAlreadyExists")
                {
                    println!("Error creating bucket {}: {}", bucket_name, e);
                }
            }
        }
    }

    // Publish a sample module from integration tests
    let handler = GenericCloudHandler::default().await;

    // Publish fake provider (aws-5) first
    let provider_path = "integration-tests/providers/aws-5";
    if std::path::Path::new(provider_path).exists() {
        println!("Publishing sample provider: {}", provider_path);
        // Using version "5.31.0" or similar as typical for aws provider
        // But integration tests use 0.1.2 sometimes. Let's try 5.0.0 to act like a real one?
        // Wait, module/lockfile might expect something specific.
        // Let's use a generic version.
        match publish_provider(&handler, provider_path, Some("5.31.0")).await {
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
