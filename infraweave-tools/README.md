# infraweave-tools

Agent tool registry for InfraWeave.

Each tool is intent-shaped (`debug_deployment`, `diff_module_versions`, ...) instead of a thin wrapper over a REST endpoint, and returns markdown-formatted summaries optimised for LLM consumption rather than raw API JSON.

The same registry is consumed by:

- [`infraweave-chat`](../infraweave-chat) - Bedrock / Anthropic / OpenAI tool use in the website chat backend.
- [`infraweave-mcp`](../infraweave-mcp) - stdio MCP server for IDE clients.

## Tools

| Name | Purpose |
|---|---|
| `list_modules` | Latest version of every published module, optionally filtered by track or name substring. |
| `describe_module` | Inputs, outputs, required providers for a specific module version. |
| `diff_module_versions` | Added / removed / changed manifest fields between two versions. |
| `list_stacks` | Latest version of every published stack. |
| `describe_stack` | Component modules + inputs/outputs of a stack. |
| `list_deployments` | Deployments in a project/region; optionally filter by module or failure state. |
| `debug_deployment` | Status + recent events + error text in one shot. |
| `list_projects` | Projects visible to the caller. |

Add a new tool by creating it under [`src/tools/`](src/tools), implementing `Tool`, and registering it in [`src/tools/mod.rs`](src/tools/mod.rs).

## Library usage

```rust
use infraweave_tools::{registry, ApiClient, ToolContext};
use serde_json::json;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Endpoint + bearer token are passed in explicitly - no file-based config.
    // The `infraweave-chat` backend forwards the user's JWT here so that
    // existing project-level authorization in `internal-api` still applies.
    let api = ApiClient::new("http://127.0.0.1:9090", "local")?;
    let ctx = ToolContext::new(api).with_track("dev");

    let tools = registry();
    let tool = tools.iter().find(|t| t.def().name == "list_modules").unwrap();
    let output = tool.execute(&ctx, json!({})).await?;
    println!("{output}");
    Ok(())
}
```

## Running against a local API

Start the in-tree scaffold (DynamoDB Local + MinIO + LocalStack via Docker, JWT auth disabled) - see [`internal-api/README.md`](../internal-api/README.md):

```bash
PORT=9090 cargo run -p internal-api --features local --bin internal-api-scaffold
```

Then point `ApiClient::new` at `http://127.0.0.1:9090` with any token (the scaffold sets `INFRAWEAVE_SKIP_AUTH=true`).
