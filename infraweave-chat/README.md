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
  "page_context": "Current page: module details for s3bucket-simple",
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
| `text` | `text` | Assistant text delta. |
| `reasoning` | `text` | Summarised model reasoning delta. |
| `tool_call` | `id`, `name`, `input` | Before tool execution. |
| `tool_result` | `id`, `name`, `output_preview`, `is_error` | After tool execution. |
| `history_append` | `message` | Canonical transcript entry — store verbatim, send back as `history`. |
| `usage` | `input_tokens`, `output_tokens`, `cache_read_input_tokens`, `cache_write_input_tokens` | Once per model call. |
| `truncated` | `reason` | Iteration cap hit. |
| `error` | `message` | Fatal error. |
| `done` | — | Stream closed. |

### History contract

Clients must preserve the transcript for the next turn:

1. **Store `history_append` verbatim** — Each event carries a canonical `LlmMessage` to append to the conversation record. Store and replay these as-is; never construct `LlmMessage` values yourself.
2. **Return as `history` next turn** — Pass the stored messages back in the `history` field on the next request. This ensures the LLM sees the full context.
3. **Use `page_context` for per-page state** — Rather than embedding page details into `message`, send them as the optional `page_context` field. The backend injects this into the user turn without affecting the system prompt, preserving cache stability.

### Model constraints

**Sonnet 5 limitations:**

- `budget_tokens` is not supported and will be rejected by Bedrock.
- Mid-conversation system messages are unsupported; the system prompt is always the first message.
- The system prompt must remain byte-stable across calls to preserve prompt caching benefits.

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
BEDROCK_MODEL_ID=us.anthropic.claude-sonnet-5 \
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
| `BEDROCK_MODEL_ID` | `us.anthropic.claude-sonnet-5` | Override to a model you have access to in your region. |
| `AWS_REGION` | *(SDK default)* | Region for Bedrock calls. |
| `PORT` | `8090` | HTTP port for `/chat`. |
| `RUST_LOG` | - | e.g. `infraweave_chat=debug`. |
