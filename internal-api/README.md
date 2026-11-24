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
- `local_setup.rs` - Local DynamoDB/MinIO container setup (feature `local`)
- `common.rs` - CloudRuntime detection and utilities

## Build

Images are assembled from prebuilt musl binaries via the bake targets in
[`bake.hcl`](bake.hcl):

```bash
# AWS Lambda (linux/arm64 or linux/amd64)
./build_image.sh internal-api-aws-unified linux/arm64
```

The Azure Functions image is the `internal-api-azure` target in the same bake
file.

## Local Development

### Quick Start

**1. Start the local server** (starts DynamoDB Local, MinIO, LocalStack, and Lambda containers via Docker):

```bash
LOCAL_INFRA=1 PORT=9090 cargo run -p internal-api --features local,swagger --bin internal-api-scaffold
```

> `LOCAL_INFRA=1` enables the embedded containers. Omit it to keep the auth bypass while
> connecting to real cloud resources instead — see [Connecting to real cloud resources](#connecting-to-real-cloud-resources).

> **API docs:** build with the `swagger` feature to serve interactive Swagger UI at
> `http://127.0.0.1:<PORT>/swagger-ui` (raw spec at `/api-docs/openapi.json`). The feature is
> orthogonal to `aws`/`azure`/`local`, so it can be combined with any of them (e.g.
> `--features local,swagger`) or left off entirely. Add `swagger` to the `--features` list of
> whichever binary you run.

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
cargo run -p cli -- module publish dev ./integration-tests/modules/s3bucket-dev --version 0.1.2-dev+test.10
```

And a newer version:

```bash
cargo run -p cli -- module publish dev ./integration-tests/modules/s3bucket-dev --version 0.1.3-dev+test.10
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

- **DynamoDB Local** on port 8000: stores modules, providers, deployments, events, etc.
- **MinIO** (S3-compatible) on port 9000: stores module/provider zip artifacts
- **LocalStack** on port 4566: provides AWS service emulation for Terraform
- **Lambda containers** on ports 8080/8081: emulate Lambda function execution
- **HTTP API** on the configured `PORT` (default 3000)

All DynamoDB tables and S3 buckets are automatically created and seeded on startup. The scaffold also publishes a sample provider (`aws-5` v0.1.2) and module (`s3bucket-simple` v1.0.0 on the `stable` track) from the integration tests so there is data to query immediately.

### HTTP mode

When the CLI is configured with an API endpoint (via `infraweave login` or the `INFRAWEAVE_API_ENDPOINT` env var), it operates in **HTTP mode**: all operations go through the HTTP API instead of calling cloud provider SDKs directly.

In HTTP mode, the CLI cannot auto-discover project and region from the cloud provider, so they must be provided explicitly:

- **`--project <id>`**: required for most commands (plan, apply, destroy, get-claim, deployments, etc.)
- **`--region <region>`**: required for commands that operate on existing deployments (destroy, driftcheck, get-claim, deployments describe, admin). For plan/apply, the region is read from the claim YAML. Falls back to the `AWS_REGION` environment variable if set.

### Environment variables

| Variable | Description | Default |
|---|---|---|
| `PORT` | HTTP server port | `3000` |
| `INFRAWEAVE_API_ENDPOINT` | Alternative to `infraweave login` for setting the API endpoint | *(optional)* |
| `LOCAL_INFRA` | When `1`/`true`, start embedded DynamoDB/MinIO containers; otherwise use the cloud resources from the environment (auth stays bypassed) | `false` |
| `LOCAL_ALLOWED_PROJECTS` | *(local builds only)* Comma-separated project IDs the local user may access. Unset or `*` allows **all** projects. Stands in for the JWT `allowed_projects` claim, which local builds have no token to carry | *(all projects)* |
| `AWS_REGION` | Fallback region when `--region` is not provided in HTTP mode | *(optional)* |

> **Auth in local builds:** any binary compiled with `--features local` bypasses JWT
> authentication entirely (see `src/http_authz.rs`). It is a compile-time flag, so the
> bypass can never reach a production binary. There is no runtime auth toggle.

### Connecting to real cloud resources

The `local` feature controls auth; `LOCAL_INFRA` controls the data source. To keep the
auth bypass but talk to **real** AWS resources instead of the embedded containers, leave
`LOCAL_INFRA` unset and provide the cloud config in the environment:

```bash
# .env is a reference file — it is NOT auto-loaded. Source it (or export the vars) first.
set -a && source internal-api/.env && set +a

# Auth bypassed (feature local), real AWS data (LOCAL_INFRA unset)
PORT=9090 cargo run -p internal-api --features local,swagger --bin internal-api-local
```

> **Warning:** the committed `.env` points at the **prod** environment
> (`INFRAWEAVE_ENVIRONMENT=prod`) and uses `AWS_PROFILE=central` with `CLOUD_PROVIDER=aws`.
> You will be reading and writing real prod tables/buckets — adjust those values for a
> non-prod target before running. With `LOCAL_INFRA` unset, `start_local_infrastructure`
> does not run, so the dummy MinIO credentials are not injected and the AWS SDK uses your
> profile/credentials as normal.

### Notes

- Data is **ephemeral**: restarting the server creates fresh containers and all published modules/providers are lost.
- The scaffold requires Docker to be running.
- Use `Ctrl+C` to gracefully shut down the server and stop all containers.

## HTTP API

All routes return JSON. See [API_EXAMPLES.md](./API_EXAMPLES.md).

Routes under `/api/v1/deployment*`, `/api/v1/deployments*`, `/api/v1/plan*`, `/api/v1/logs*`, `/api/v1/events*`, `/api/v1/change_record*`, `/api/v1/change_record_graph*`, `/api/v1/deployment_graph*`, `/api/v1/cost*`, `/api/v1/costs*`, `/api/v1/cost_resources*`, `/api/v1/cost_tags*`, `/api/v1/job_status*`, `/api/v1/provider/download`, `/api/v1/apply`, `/api/v1/destroy`, and `/api/v1/reapply` require project-level JWT authorization.

Publish and deprecate routes (`/api/v1/module/publish`, `/api/v1/stack/publish`, `/api/v1/provider/publish`, and `*/deprecate`) require publish-level JWT authorization. Publish rights are derived from CI/CD identity-provider claims (GitHub Actions OIDC today) and evaluated by the Rego publish policy.

The full design, configuration reference, and GitHub OIDC examples for single-repo and monorepo layouts live in [src/publish_auth/README.md](src/publish_auth/README.md).

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
- `GET /api/v1/cost/{project}/{region}/*rest?granularity=DAILY&periods=30&group_by=SERVICE` *(see Deployment cost below)*
- `GET /api/v1/costs/{project}/{region}?granularity=DAILY&periods=30&group_by=SERVICE` *(cost of every deployment in the account; see Deployment cost below)*
- `GET /api/v1/cost_resources/{project}/{region}?days=14&deployment_ids=...` *(cost per resource; see Deployment cost below)*
- `GET /api/v1/cost_tags/{project}/{region}?days=90` *(cost allocation tag keys to group and filter by; see Deployment cost below)*
- `POST /api/v1/apply` *(auth required; JSON body with `project`/`project_id`, `environment`/`environment_id`, and `claim`/`manifest`)*
- `POST /api/v1/plan` *(auth required; JSON body with `project`/`project_id`, `environment`/`environment_id`, and `claim`/`manifest`)*
- `POST /api/v1/destroy` *(auth required; JSON body with `project`/`project_id`, `region`, `environment`/`environment_id`, and `deployment_id`)*
- `POST /api/v1/reapply` *(auth required; JSON body with `project`/`project_id`, `region`, `environment`/`environment_id`, and `deployment_id`)*

**Deployment cost:**

`GET /api/v1/cost/{project}/{region}/{env1}/{env2}/{dep1}/{dep2}` returns the deployment's AWS cost from Cost Explorer, for the resources tagged `INFRAWEAVE_DEPLOYMENT_ID=<deployment_id>`. The API assumes `infraweave_api_read_log-<environment>` in the project account and calls `ce:GetCostAndUsage` there, so a project only sees its own account's spend.

| Query parameter | Default | Notes |
| --- | --- | --- |
| `granularity` | `DAILY` | `MONTHLY`, `DAILY` or `HOURLY`. Hourly needs hourly granularity enabled in Cost Explorer and covers the last 14 days only. |
| `periods` | 6 / 30 / 24 | Periods back from (and including) the current one, used when `start` is not set. |
| `start`, `end` | | `YYYY-MM-DD` or RFC 3339. `end` is exclusive and defaults to the end of the current period. |
| `group_by` | `SERVICE` | Up to two comma-separated Cost Explorer dimensions (`SERVICE`, `USAGE_TYPE`, `OPERATION`, `REGION`, `AZ`, `INSTANCE_TYPE`, `PURCHASE_TYPE`, ...). Empty for totals only. |
| `metric` | `AmortizedCost` | `UnblendedCost`, `BlendedCost`, `NetAmortizedCost` or `NetUnblendedCost`. Amortized spreads Savings Plan and Reserved Instance commitments over the usage they cover, so covered usage does not show as nearly free; without commitments it equals unblended. |

The response holds `total`, `groups` (the whole range per dimension value, largest first) and `periods` (each with `start`, `end`, `estimated`, `total` and its `groups`). Answers are cached for 5 hours per Lambda instance, and sent with `Cache-Control: private, max-age=18000` so the browser reuses them too: Cost Explorer only refreshes a few times a day and bills each request.

`GET /api/v1/costs/{project}/{region}` returns the cost of every deployment in one account from a single Cost Explorer query. It takes the same `granularity`, `periods`, `start`, `end` and `metric` as above, plus:

| Query parameter | Notes |
| --- | --- |
| `group_by` | One dimension (`SERVICE`, `REGION`, `USAGE_TYPE`, ...) or `tag:<key>` (a cost allocation tag; costs without it get an empty value) to break each deployment's cost down by. None by default. |
| `deployment_ids`, `services`, `regions` | Comma-separated values to narrow to. Empty entries are ignored. |
| `modules` | Comma-separated modules, matched against the first segment of the deployment id (`module/name`). Applied to Cost Explorer's answer rather than sent with the query, so destroyed deployments of the module are kept too, unlike a list of `deployment_ids` taken from the live ones. |
| `tag_filters` | A JSON object of cost allocation tag key to values to narrow to, e.g. `{"team":["platform","data"]}`; keys are ANDed, values ORed. Up to 10 keys. `INFRAWEAVE_DEPLOYMENT_ID` is filtered on with `deployment_ids` instead. |
| `series` | `false` for totals only: the range is read in monthly periods, which costs fewer billed pages. |

The query is always grouped by the `INFRAWEAVE_DEPLOYMENT_ID` tag first and untagged costs are dropped before anything is summed, so only deployment costs are ever returned, whatever the grouping or filters. The response has `total` and `deployments` (per deployment), and with `group_by` also `groups` (per dimension value) and `breakdown` (per deployment and value); `periods` holds the same per period. It takes one project per request, and the costs cover the whole account whichever region is in the path.

`GET /api/v1/cost_tags/{project}/{region}` lists the cost allocation tag keys with cost in the account over the last `days` (90 by default), the ones Cost Explorer offers to group by. Only keys are listed: values would include those of resources outside any deployment, so they are read with `group_by=tag:<key>` instead, which only counts deployment costs.

`GET /api/v1/cost_resources/{project}/{region}` returns the cost per resource (`resources`: `{ deployment_id, resource_id, amount }`) over the last `days` (1 to 14, the most Cost Explorer keeps), from `GetCostAndUsageWithResources`. It needs `deployment_ids` or `services` to narrow it to, takes the same filters as above (including `tag_filters`), and only works once resource-level data is enabled in the Cost Explorer settings of the management account.

`INFRAWEAVE_DEPLOYMENT_ID` must be activated as a cost allocation tag in the organization's management (payer) account, and member accounts must be allowed to use Cost Explorer. Costs from before the tag was activated are not attributed, and new costs show up with a delay of up to 24 hours.

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
- `local` - Local development mode: bypasses JWT authentication and enables direct DB access. Starts embedded DynamoDB/MinIO containers only when `LOCAL_INFRA` is set; otherwise connects to the cloud resources from the environment
- `swagger` - Serves interactive Swagger UI at `/swagger-ui` (spec at `/api-docs/openapi.json`). Orthogonal to the above; combine with any of them or use standalone

`aws` and `azure` are mutually exclusive. `local` can be combined with `aws` for local development.
