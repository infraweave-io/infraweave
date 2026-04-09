# Env AWS Direct

This crate implements the `CloudProvider` trait for AWS using **direct AWS SDK calls** (DynamoDB, S3, ECS, CloudWatch, SNS) instead of invoking Lambda functions. It is a separate crate from `env_aws`, which routes operations through Lambda.

## When to Use

Use `env_aws_direct` instead of `env_aws` when you want to:
- Run the internal API locally without Lambda
- Reduce latency by eliminating Lambda invocation overhead
- Perform cross-account ECS/CloudWatch operations

## Usage

```toml
[dependencies]
env_aws_direct = { path = "../env_aws_direct" }
```

## Exported Functions

### Direct Database & Storage Operations

- `read_db_direct` - Query DynamoDB directly
- `insert_db_direct` - Insert items into DynamoDB directly
- `transact_write_direct` - Transactional DynamoDB write
- `upload_file_base64_direct` - Upload base64-encoded content to S3
- `upload_file_url_direct` - Download from URL and upload to S3
- `download_file_as_string_direct` - Download S3 object as string
- `download_file_as_bytes_direct` - Download S3 object as bytes
- `generate_presigned_url_direct` - Generate S3 presigned URL
- `get_environment_variables_direct` - Return environment variables
- `publish_notification_direct` - Publish to SNS topic

### Cross-Account Operations (via STS Assume Role)

- `start_runner_cross_account` - Launch ECS task in target account
- `get_job_status_cross_account` - Get ECS task status across accounts
- `read_logs_cross_account` - Read CloudWatch logs from different accounts

### Query Builders (from `api.rs`)

Functions that build DynamoDB query payloads for use with `read_db_direct`:

- `get_deployment_query`, `get_deployment_and_dependents_query`, `get_dependents_query`
- `get_all_deployments_query`, `get_deployments_using_module_query`, `get_deployments_to_driftcheck_query`
- `get_deployment_history_deleted_query`, `get_deployment_history_plans_query`, `get_plan_deployment_query`
- `get_all_latest_modules_query`, `get_module_version_query`, `get_latest_module_version_query`, `get_all_module_versions_query`
- `get_all_latest_stacks_query`, `get_stack_version_query`, `get_latest_stack_version_query`, `get_all_stack_versions_query`
- `get_all_latest_providers_query`, `get_provider_version_query`, `get_latest_provider_version_query`
- `get_all_policies_query`, `get_policy_query`, `get_newest_policy_version_query`
- `get_events_query`, `get_all_events_between_query`
- `get_change_records_query`
- `get_all_projects_query`, `get_current_project_query`, `get_project_map_query`
- `get_all_regions_query`

### HTTP Authentication

- `call_authenticated_http` - SigV4-signed HTTP request
- `call_authenticated_http_raw` - SigV4-signed HTTP request (raw response)
- `call_authenticated_http_with_config` - SigV4-signed HTTP with custom config
- `get_aws_auth_context` - Returns `(has_credentials, region)`

### Other

- `AwsCloudProvider` - `CloudProvider` trait implementation
- `set_backend` - Configure Terraform backend (S3 + DynamoDB)
- `get_current_job_id` - Get current ECS task ID
- `get_region` - Get AWS region from environment
- `get_table_name_for_region` / `get_bucket_name` / `get_bucket_name_for_region` - Resource name resolution
- `bootstrap_dynamodb_tables` / `create_s3_buckets` - Local development setup

## Required Environment Variables

### Core (DynamoDB table names)

- `DYNAMODB_EVENTS_TABLE_NAME` - Events table
- `DYNAMODB_MODULES_TABLE_NAME` - Modules table
- `DYNAMODB_DEPLOYMENTS_TABLE_NAME` - Deployments table
- `DYNAMODB_POLICIES_TABLE_NAME` - Policies table
- `DYNAMODB_CHANGE_RECORDS_TABLE_NAME` - Change records table
- `DYNAMODB_CONFIG_TABLE_NAME` - Config table
- `AWS_REGION` - AWS region

### Cross-Account ECS/CloudWatch

- `ECS_CLUSTER` - ECS cluster name
- `ENVIRONMENT` - Environment name
- `CENTRAL_ACCOUNT_ID` - Central AWS account ID
- `NOTIFICATION_TOPIC_ARN` - SNS topic ARN for notifications

### Local Development (`TEST_MODE`)

When `TEST_MODE` is set, the crate uses local endpoints instead of real AWS services:

- `DYNAMODB_ENDPOINT` or `AWS_ENDPOINT_URL_DYNAMODB` - Local DynamoDB endpoint
- `AWS_ENDPOINT_URL_S3` or `MINIO_ENDPOINT` - Local S3/MinIO endpoint
- `AWS_S3_FORCE_PATH_STYLE` - Force path-style S3 URLs (for MinIO)
- `AWS_ACCESS_KEY_ID` / `AWS_SECRET_ACCESS_KEY` - Credentials for local services
