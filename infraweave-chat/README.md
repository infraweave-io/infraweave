# infraweave-chat

HTTP chat backend for an InfraWeave website chatbot.

```
browser -> POST /chat (SSE) -> infraweave-chat
                                |
                                +-> Bedrock Converse
                                |
                                +-> infraweave-tools -> internal-api
```

The same tool registry powers [`infraweave-mcp`](../infraweave-mcp) for IDE clients.

## Endpoint

`POST /chat` - Server-Sent Events.

Request body:

```json
{
  "message": "describe the s3bucket-simple module on the dev track",
  "history": [],
  "project": "...",
  "region": "...",
  "environment": "...",
  "track": "dev"
}
```

Headers: `Authorization: Bearer <user-jwt>`. The token is forwarded to `internal-api`, where project authorization is enforced.

SSE event payloads (one JSON object per `data:` line):

| `type` | Fields | When |
|---|---|---|
| `tool_call` | `name`, `input` | LLM decided to call a tool (before execution). |
| `tool_result` | `name`, `output_preview`, `is_error` | Tool finished. Full output is fed back to the LLM. |
| `text` | `text` | Assistant text chunk. |
| `truncated` | `reason` | Iteration cap hit. |
| `error` | `message` | Fatal error. |
| `done` | - | Stream closed. |

## Local run

### 1. Local InfraWeave API

See [`internal-api/README.md`](../internal-api/README.md). With Docker running:

```bash
PORT=9090 cargo run -p internal-api --features local --bin internal-api-scaffold
```

Auth is skipped, so any bearer token works. The scaffold seeds a sample provider and `s3bucket-simple` module.

### 2. Bedrock prerequisites

- AWS credentials available to the SDK (`aws sso login` or `AWS_ACCESS_KEY_ID`/`AWS_SECRET_ACCESS_KEY`).
- Enable model access in the Bedrock console. Without this, the first call returns `AccessDeniedException`.
- Pick a model ID you actually have access to in your region. Confirm with:
  ```bash
  aws bedrock list-foundation-models --region us-west-2 \
    --query 'modelSummaries[?contains(modelId, `anthropic`)].modelId' --output text
  ```

### 3. Start the chat backend

```bash
INFRAWEAVE_API_ENDPOINT=http://127.0.0.1:9090 \
BEDROCK_MODEL_ID=us.amazon.nova-pro-v1:0 \
AWS_REGION=us-west-2 \
RUST_LOG=infraweave_chat=info \
cargo run -p infraweave-chat
```

### 4. Hit it

```bash
curl -N -H "Authorization: Bearer local" \
     -H "Content-Type: application/json" \
     http://localhost:8090/chat \
     -d '{"message":"what modules are published?"}'
```

`-N` disables curl buffering so SSE frames stream as they arrive. Expected: one or more `tool_call` / `tool_result` events, then `text`, then `done`.

A second test that exercises a tool requiring a session default:

```bash
curl -N -H "Authorization: Bearer local" \
     -H "Content-Type: application/json" \
     http://localhost:8090/chat \
     -d '{
       "message": "describe the s3bucketsimple module",
       "track": "stable"
     }'
```

## Lambda image

`infraweave-chat/Dockerfile.lambda` builds a Lambda container image for the chat backend.

Build from the workspace root:

```bash
docker build -f infraweave-chat/Dockerfile.lambda -t infraweave-chat-lambda .
```

Runtime environment:

| Var | Notes |
|---|---|
| `INFRAWEAVE_API_ENDPOINT` | Required. URL of `internal-api` / API Gateway. |
| `BEDROCK_MODEL_ID` | Optional. Override to a model the chat Lambda role can invoke. |
| `AWS_REGION` | Region for Bedrock calls. |
| `RUST_LOG` | e.g. `infraweave_chat=info`. |

The Lambda needs Bedrock invoke permissions and network access to `internal-api`. The caller's bearer token is still passed through to `internal-api`; chat does not need direct InfraWeave data-plane permissions.

The image defaults the Lambda Web Adapter to `AWS_LWA_INVOKE_MODE=buffered`, which works with API Gateway HTTP APIs. For Lambda response streaming, override this to `response_stream` and configure the gateway or Function URL accordingly.

## Environment variables

| Var | Default | Notes |
|---|---|---|
| `INFRAWEAVE_API_ENDPOINT` | *(required)* | URL of `internal-api` (local scaffold or API Gateway). |
| `BEDROCK_MODEL_ID` | `us.amazon.nova-pro-v1:0` | Override to a model you have access to in your region. |
| `AWS_REGION` | *(SDK default)* | Region for Bedrock calls. |
| `PORT` | `8090` | HTTP port for `/chat`. |
| `RUST_LOG` | - | e.g. `infraweave_chat=debug`. |
