# Internal API

Multi-cloud serverless API for Infraweave. Supports HTTP (preferred) and native function invocation (AWS/Azure legacy).

## Architecture

**Binaries:**
- `internal-api-aws-unified` - AWS Lambda (native invocation + HTTP via API Gateway)
- `internal-api-azure-unified` - Azure Functions (native invocation + HTTP)
- `internal-api-local` - Local HTTP server for development
- `internal-api-scaffold` - Local development scaffold with DynamoDB/MinIO/LocalStack containers (feature `local`)

**Modules:**
- `lib.rs` - Module exports and router
- `api_common.rs` - Common API implementations (DatabaseQuery trait)
- `aws_handlers.rs` - AWS operations (DynamoDB, S3, ECS, CloudWatch, SNS) (feature `aws`)
- `azure_handlers.rs` - Azure operations (Cosmos DB, Blob Storage, ACI, Azure Monitor) (feature `azure`)
- `http_router.rs` - Axum HTTP router
- `auth_handler.rs` - JWT authentication and token validation
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

### Quick Start

**1. Start the local server** (starts DynamoDB Local, MinIO, LocalStack, and Lambda containers via Docker):

```bash
PORT=9090 cargo run -p internal-api --features local --bin internal-api-scaffold
```

**2. In a separate terminal, configure the CLI** to point at the local server:

```bash
cargo run -p cli login --api-endpoint http://127.0.0.1:9090
```

This stores the API endpoint in `~/.infraweave/tokens.json`. All subsequent CLI commands will use HTTP mode automatically.

> **Note:** The local scaffold disables JWT authentication, so the login will succeed without real credentials. For CI or scripting, you can alternatively set the `INFRAWEAVE_API_ENDPOINT` environment variable instead of running `login`.

**3. Publish a provider:**

```bash
cargo run -p cli -- provider publish ./integration-tests/providers/aws-5 --version 0.1.3
```

**4. Publish a module:**

```bash
cargo run -p cli -- module publish dev ./integration-tests/modules/s3bucket-simple --version 1.2.3-dev
```

**5. Publish a stack:**

```bash
cargo run -p cli -- stack publish dev ./integration-tests/stacks/bucketcollection-dev --version 0.0.1-dev
```

**6. List published resources:**

```bash
cargo run -p cli -- provider list
cargo run -p cli -- module list dev
cargo run -p cli -- stack list dev
```

**7. Plan a deployment** (requires `--project` flag in HTTP mode):

```bash
cargo run -p cli -- plan integration-tests/claims/s3bucket-dev-claim.yaml --project 123456789012
```

Commands that operate on existing deployments (e.g. `destroy`, `get-claim`, `deployments describe`) also require `--region`:

```bash
cargo run -p cli -- deployments list --project 123456789012 --region us-west-2
cargo run -p cli -- get-claim --project 123456789012 --region us-west-2
```

### What the scaffold does

The `internal-api-scaffold` binary (behind the `local` feature) uses the integration test scaffold to spin up a fully functional local environment:

- **DynamoDB Local** on port 8000 — stores modules, providers, deployments, events, etc.
- **MinIO** (S3-compatible) on port 9000 — stores module/provider zip artifacts
- **LocalStack** on port 4566 — provides AWS service emulation for Terraform
- **Lambda containers** on ports 8080/8081 — emulate Lambda function execution
- **HTTP API** on the configured `PORT` (default 3000)

All DynamoDB tables and S3 buckets are automatically created and seeded on startup. The scaffold also publishes a sample provider (`aws-5` v0.1.2) and module (`s3bucket-simple` v1.0.0 on the `stable` track) from the integration tests so there is data to query immediately.

### HTTP mode

When the CLI is configured with an API endpoint (via `infraweave login` or the `INFRAWEAVE_API_ENDPOINT` env var), it operates in **HTTP mode**: all operations go through the HTTP API instead of calling cloud provider SDKs directly.

In HTTP mode, the CLI cannot auto-discover project and region from the cloud provider, so they must be provided explicitly:

- **`--project <id>`** — required for most commands (plan, apply, destroy, get-claim, deployments, etc.)
- **`--region <region>`** — required for commands that operate on existing deployments (destroy, driftcheck, get-claim, deployments describe, admin). For plan/apply, the region is read from the claim YAML. Falls back to the `AWS_REGION` environment variable if set.

### Environment variables

| Variable | Description | Default |
|---|---|---|
| `PORT` | HTTP server port | `3000` |
| `INFRAWEAVE_API_ENDPOINT` | Alternative to `infraweave login` for setting the API endpoint | *(optional)* |
| `INFRAWEAVE_SKIP_AUTH` | Skip JWT authentication (used by scaffold) | `false` |
| `AWS_REGION` | Fallback region when `--region` is not provided in HTTP mode | *(optional)* |

### Notes

- Data is **ephemeral** — restarting the server creates fresh containers and all published modules/providers are lost.
- The scaffold requires Docker to be running.
- Use `Ctrl+C` to gracefully shut down the server and stop all containers.

## HTTP API

All routes return JSON. See [API_EXAMPLES.md](./API_EXAMPLES.md).

Routes under `/api/v1/deployment*`, `/api/v1/deployments*`, `/api/v1/plan*`, `/api/v1/logs*`, `/api/v1/events*`, `/api/v1/change_record*`, `/api/v1/change_record_graph*`, `/api/v1/deployment_graph*`, `/api/v1/job_status*`, `/api/v1/provider/download`, and `/api/v1/claim/run` require project-level JWT authorization.

Publish and deprecate routes (`/api/v1/module/publish`, `/api/v1/stack/publish`, `/api/v1/provider/publish`, and `*/deprecate`) require publish-level JWT authorization via the `custom:publish_permissions` claim.

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
- `PUT /api/v1/module/{track}/{module}/{version}/deprecate` *(publish auth)*
- `POST /api/v1/module/publish` *(publish auth)*
- `GET /api/v1/stacks`
- `GET /api/v1/stack/{track}/{stack_name}/{stack_version}`
- `GET /api/v1/stack/{track}/{stack_name}/{stack_version}/download`
- `GET /api/v1/stacks/versions/{track}/{stack}`
- `PUT /api/v1/stack/{track}/{stack}/{version}/deprecate` *(publish auth)*
- `POST /api/v1/stack/publish` *(publish auth)*

**Providers:**
- `GET /api/v1/providers`
- `GET /api/v1/provider/{track}/{provider}/{version}`
- `GET /api/v1/provider/{track}/{provider}/{version}/download`
- `POST /api/v1/provider/download` *(auth required)*
- `POST /api/v1/provider/publish` *(publish auth)*

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

## Native Invocation (AWS Legacy)

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
- `local` - Local development mode: starts embedded DynamoDB/MinIO containers and enables direct DB access

`aws` and `azure` are mutually exclusive. `local` can be combined with `aws` for local development.
