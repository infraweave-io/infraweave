# Env AWS

This package implements the trait CloudProvider for the AWS cloud provider.

## Features

### Direct Database Access

The `direct` feature flag enables direct DynamoDB access instead of going through Lambda functions. This is useful for:
- Local development and testing
- Running the internal API directly without Lambda
- Reducing latency by eliminating the Lambda invocation overhead

#### Usage

To enable direct database access, add the feature to your dependency:

```toml
[dependencies]
env_aws = { path = "../env_aws", features = ["direct"] }
```

Or when building:

```bash
cargo build --features direct
```

#### Example

```rust
use env_aws::{execute_get_modules, execute_read_db};

// With the "direct" feature enabled, these functions will call DynamoDB directly
// Without the feature, they will invoke Lambda functions
let modules = execute_get_modules(&project_id, &region).await?;
let deployments = execute_get_deployments(&project_id, &region, &environment).await?;
```

#### Required Environment Variables (for direct mode)

When using the `direct` feature, ensure these environment variables are set:

- `DYNAMODB_EVENTS_TABLE_NAME` - Name of the events table
- `DYNAMODB_MODULES_TABLE_NAME` - Name of the modules table  
- `DYNAMODB_DEPLOYMENTS_TABLE_NAME` - Name of the deployments table
- `DYNAMODB_POLICIES_TABLE_NAME` - Name of the policies table
- `DYNAMODB_CHANGE_RECORDS_TABLE_NAME` - Name of the change records table
- `DYNAMODB_CONFIG_TABLE_NAME` - Name of the config table

#### Available Execute Functions

All 19 functions from `aws_handlers` are now supported with both direct and Lambda-based execution:

**Database Query Functions (Read-Only):**
- `execute_read_db` - Generic database query
- `execute_describe_deployment` - Describe a deployment and its dependents
- `execute_get_deployments` - Get deployments for a region/environment
- `execute_get_modules` - Get all modules
- `execute_get_stacks` - Get all stacks
- `execute_get_projects` - Get all projects (requires central role)
- `execute_get_policies` - Get policies for an environment
- `execute_get_policy_version` - Get specific policy version
- `execute_get_module_version` - Get specific module version
- `execute_get_stack_version` - Get specific stack version
- `execute_get_all_versions_for_module` - Get all versions of a module
- `execute_get_all_versions_for_stack` - Get all versions of a stack
- `execute_get_deployments_for_module` - Get deployments using a module
- `execute_get_events` - Get events for a deployment
- `execute_get_change_record` - Get change record for a deployment

**AWS Service Functions:**
- `execute_get_job_status` - Get ECS task status
- `execute_read_logs` - Read CloudWatch logs for a job
- `execute_get_environment_variables` - Get environment variables

**Write Operations:**
- `execute_deprecate_module` - Mark a module version as deprecated

#### Additional Environment Variables (for ECS/CloudWatch)

When using functions that interact with ECS or CloudWatch Logs:

- `ECS_CLUSTER` - Name of the ECS cluster (for job status)
- `DYNAMODB_TF_LOCKS_TABLE_ARN` - DynamoDB table ARN for Terraform locks
- `TF_STATE_S3_BUCKET` - S3 bucket for Terraform state
- `REGION` - AWS region
