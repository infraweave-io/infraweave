// Local development bootstrap - creates DynamoDB tables and S3 buckets for local testing
use anyhow::Result;
use std::collections::HashMap;

pub async fn bootstrap_dynamodb_tables(dynamodb_endpoint: &str, region: &str) -> Result<()> {
    use aws_sdk_dynamodb::types::{
        AttributeDefinition, BillingMode, GlobalSecondaryIndex, KeySchemaElement, KeyType,
        Projection, ProjectionType, ScalarAttributeType,
    };

    let creds =
        aws_sdk_dynamodb::config::Credentials::new("minio", "minio123", None, None, "static");
    let config = aws_sdk_dynamodb::Config::builder()
        .behavior_version(aws_sdk_dynamodb::config::BehaviorVersion::latest())
        .endpoint_url(dynamodb_endpoint)
        .region(aws_sdk_dynamodb::config::Region::new(region.to_string()))
        .credentials_provider(creds)
        .build();
    let dynamodb = aws_sdk_dynamodb::Client::from_conf(config);

    let table_map: HashMap<&str, &str> =
        crate::utils::DEFAULT_TABLE_NAMES.iter().copied().collect();

    for (env_var, table_name) in &table_map {
        std::env::set_var::<&str, &str>(env_var, table_name);
        println!("Creating DynamoDB table: {}", table_name);

        let mut create_table = dynamodb
            .create_table()
            .table_name(table_name.to_string())
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
                        .index_name("ReverseIndex")
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

        match create_table.send().await {
            Ok(_) => println!("Table {} created", table_name),
            Err(e) => {
                let err_str = e.to_string();
                if !err_str.contains("ResourceInUseException") {
                    println!("Error creating table {}: {}", table_name, e);
                }
            }
        }
    }

    // Seed config table
    let config_table_name = table_map.get("DYNAMODB_CONFIG_TABLE_NAME").unwrap();
    println!("Seeding Config Table: {}", config_table_name);

    let regions_val = aws_sdk_dynamodb::types::AttributeValue::L(vec![
        aws_sdk_dynamodb::types::AttributeValue::S(region.to_string()),
    ]);
    let data_map = aws_sdk_dynamodb::types::AttributeValue::M(HashMap::from([(
        "regions".to_string(),
        regions_val,
    )]));

    let _ = dynamodb
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
        .await;

    // Seed sample projects
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
        let regions_list = vec![aws_sdk_dynamodb::types::AttributeValue::S(
            region.to_string(),
        )];

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

    Ok(())
}

pub async fn create_s3_buckets(s3_endpoint: &str, region: &str) -> Result<()> {
    let creds = aws_sdk_s3::config::Credentials::new("minio", "minio123", None, None, "static");
    let config = aws_sdk_s3::Config::builder()
        .behavior_version(aws_sdk_s3::config::BehaviorVersion::latest())
        .endpoint_url(s3_endpoint)
        .region(aws_sdk_s3::config::Region::new(region.to_string()))
        .credentials_provider(creds)
        .force_path_style(true)
        .build();
    let s3 = aws_sdk_s3::Client::from_conf(config);

    let bucket_map: HashMap<&str, &str> =
        crate::utils::DEFAULT_BUCKET_NAMES.iter().copied().collect();

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

    Ok(())
}
