# Internal API

Multi-cloud serverless API for Infraweave. Supports both direct invocation (AWS legacy) and HTTP (modern).

## Architecture

**Binaries:**
- `internal-api-aws-unified` - AWS Lambda (direct invocation + HTTP via API Gateway)
- `internal-api-azure-unified` - Azure Functions (direct invocation + HTTP)
- `internal-api-local` - Local HTTP server for development

**Modules:**
- `lib.rs` - Module exports and router
- `api_common.rs` - Common API implementations (DatabaseQuery trait)
- `aws_handlers.rs` - AWS operations (DynamoDB, S3, ECS, CloudWatch, SNS) (feature `aws`)
- `azure_handlers.rs` - Azure operations (Cosmos DB, Blob Storage, ACI, Azure Monitor) (feature `azure`)
- `http_router.rs` - Axum HTTP router
- `auth_handler.rs` - JWT authentication and token validation (feature `aws`)
- `auth_handler_tests.rs` - JWT authentication tests
- `handlers.rs` - Shared request/response handler logic
- `queries.rs` - Database query helpers
- `otel_tracing.rs` - OpenTelemetry tracing setup
- `local_setup.rs` - Local DynamoDB/MinIO container setup (feature `local`)
- `common.rs` - CloudRuntime detection and utilities

## Build

```bash
# AWS Lambda
docker build -f internal-api/Dockerfile.lambda -t internal-api-lambda .

# Azure Functions
docker build -f internal-api/Dockerfile.azure -t internal-api-azure .
```

## Local Development

Start the local test infrastructure (DynamoDB Local + MinIO):

```bash
LOG_LEVEL=info cargo run --bin internal-api-local --features local
```

This automatically:
- Starts DynamoDB Local on port 8000
- Starts MinIO (S3-compatible) on port 9000
- Creates all required tables and buckets
- Seeds sample data from `integration-tests/`
- Starts HTTP API on http://localhost:8080

Then in a separate terminal, point the CLI at the local server:

```bash
INFRAWEAVE_API_ENDPOINT=http://localhost:8080 cargo run --bin cli -- module list stable
```

The `local` feature starts the embedded DynamoDB/MinIO containers and seeds data. The CLI connects via HTTP using `INFRAWEAVE_API_ENDPOINT`.

## HTTP API

All routes return JSON. See [API_EXAMPLES.md](./API_EXAMPLES.md).

Routes under `/api/v1/deployment*`, `/api/v1/deployments*`, `/api/v1/plan*`, `/api/v1/logs*`, `/api/v1/events*`, `/api/v1/change_record*`, `/api/v1/job_status*`, `/api/v1/provider/download`, and `/api/v1/claim/run` require project-level JWT authorization.

**Deployments:**
- `GET /api/v1/deployment/{project}/{region}/*rest`
- `GET /api/v1/deployments/{project}/{region}`
- `GET /api/v1/deployments/module/{project}/{region}/{module}`
- `GET /api/v1/deployments/history/{project}/{region}`
- `GET /api/v1/plan/{project}/{region}/*rest`
- `GET /api/v1/events/{project}/{region}/*rest`
- `GET /api/v1/change_record/{project}/{region}/*rest`
- `GET /api/v1/change_record_graph/{project}/{region}/*rest`
- `GET /api/v1/deployment_graph/{project}/{region}/*rest`

**Modules & Stacks:**
- `GET /api/v1/modules`
- `GET /api/v1/module/{track}/{module_name}/{module_version}`
- `GET /api/v1/module/{track}/{module_name}/{module_version}/download`
- `GET /api/v1/modules/versions/{track}/{module}`
- `PUT /api/v1/module/{track}/{module}/{version}/deprecate`
- `POST /api/v1/module/publish`
- `GET /api/v1/module/publish/{job_id}`
- `GET /api/v1/stacks`
- `GET /api/v1/stack/{track}/{stack_name}/{stack_version}`
- `GET /api/v1/stack/{track}/{stack_name}/{stack_version}/download`
- `GET /api/v1/stacks/versions/{track}/{stack}`

**Providers:**
- `GET /api/v1/providers`
- `GET /api/v1/provider/{track}/{provider}/{version}`
- `GET /api/v1/provider/{track}/{provider}/{version}/download`
- `POST /api/v1/provider/download` *(auth required)*

**Projects & Policies:**
- `GET /api/v1/projects`
- `GET /api/v1/policies/{environment}`
- `GET /api/v1/policy/{environment}/{policy_name}/{policy_version}`

**Logs & Jobs:**
- `GET /api/v1/logs/{project}/{region}/{job_id}?limit=100&next_token=...`
- `GET /api/v1/job_status/{project}/{region}/*rest`

**Operations:**
- `POST /api/v1/claim/run` *(auth required)*

**Auth & Meta:**
- `POST /api/v1/auth/token`
- `GET /api/v1/meta`

## Direct Invocation (AWS Legacy)

For backwards compatibility with Python Lambda callers. Format: `{"event": "EVENT_NAME", ...}`

**Database:** `insert_db`, `transact_write`, `read_db`  
**Storage:** `upload_file_base64`, `upload_file_url`, `generate_presigned_url`  
**Execution:** `start_runner`, `get_job_status`, `read_logs`  
**Other:** `publish_notification`, `get_environment_variables`

## Environment Variables

**AWS:** See [.env](./.env)  
**Azure:** See [.env.azure](./.env.azure)

Key variables:
- `DYNAMODB_*_TABLE_NAME` / `COSMOS_CONTAINER_*` - Database containers
- `*_S3_BUCKET` / `STORAGE_ACCOUNT_NAME` - Object storage
- `REGION`, `ENVIRONMENT` - Infrastructure context
- `CLOUD_PROVIDER` - Optional override (auto-detected)

## Feature Flags

- `aws` - AWS Lambda support (default)
- `azure` - Azure Functions support
- `local` - Local development mode: starts embedded DynamoDB/MinIO containers and enables direct DB access (implies `env_aws/direct`)

`aws` and `azure` are mutually exclusive. `local` can be combined with `aws` for local development.
