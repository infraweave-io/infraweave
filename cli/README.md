# CLI

Command-line interface for InfraWeave. Used to publish modules, providers, and stacks, apply/destroy deployments, and inspect resources.

## Execution modes

### 1. HTTP mode (preferred)

```
CLI → HTTP → internal-api server → DynamoDB / S3
```

**Setup:** Start the local server, then log in once to store the endpoint:

```bash
PORT=9090 cargo run -p internal-api --features local --bin internal-api-scaffold
```

```bash
cargo run -p cli -- login --api-endpoint http://127.0.0.1:9090
```

This stores the endpoint in `~/.infraweave/tokens.json`. All subsequent CLI commands automatically use HTTP mode — no flags or environment variables required.

**Example commands:**

```bash
cargo run -p cli -- provider list
cargo run -p cli -- module list dev
cargo run -p cli -- stack list dev

cargo run -p cli -- provider publish ./integration-tests/providers/aws-5 --version 0.1.3
cargo run -p cli -- module publish dev ./integration-tests/modules/s3bucket-simple --version 1.2.3
cargo run -p cli -- stack publish dev ./integration-tests/stacks/bucketcollection-dev --version 0.0.1
```

Commands that work with deployments require `--project` (and `--region` for region-scoped commands):

```bash
cargo run -p cli -- plan integration-tests/claims/s3bucket-dev-claim.yaml --project 123456789012
cargo run -p cli -- deployments list --project 123456789012 --region us-west-2
```

Alternatively, set `INFRAWEAVE_API_ENDPOINT` instead of running `login`:

```bash
INFRAWEAVE_API_ENDPOINT=http://127.0.0.1:9090 cargo run -p cli -- module list dev
```

**How it works:** When an API endpoint is configured, `is_http_mode_enabled()` returns true and the CLI routes operations through the `http_client` crate's HTTP API functions directly. No cloud SDK calls are made.

### 2. Legacy (AWS)

```
CLI → Lambda function invocation → DynamoDB
```

Uses AWS IAM credentials. No login or API endpoint needed.

```bash
AWS_PROFILE=central AWS_REGION=us-west-2 cargo run -p cli -- module list dev
AWS_PROFILE=central AWS_REGION=us-west-2 cargo run -p cli -- module publish dev ./my-module stable
```

**Characteristics:**
- IAM-based authentication via Lambda invocation
- Lambda authorizes and assumes roles into workload accounts
- No token refresh logic, no API Gateway quotas

### 3. Legacy (Azure)

```
CLI → Azure Function invocation → Cosmos DB / Blob Storage
```

```bash
CLOUD_PROVIDER=azure cargo run -p cli -- module list dev
```

## Provider selection

The active cloud provider is determined by `provider_name()` in `env_common`:

1. `CLOUD_PROVIDER` or `PROVIDER` env var (explicit override)
2. HTTP mode auto-detected when `INFRAWEAVE_API_ENDPOINT` is set or `~/.infraweave/tokens.json` has an `api_endpoint` → selects `HttpCloudProvider`
3. Defaults to `aws` (legacy Lambda function invocation)

## Development

For rapid iteration against a live cloud account:

```bash
AWS_PROFILE=central AWS_REGION=us-west-2 cargo run -p cli -- <COMMAND> <ARG1> ...
```

For local development, use the `internal-api-scaffold` as described in [HTTP mode](#1-http-mode-preferred) above.
