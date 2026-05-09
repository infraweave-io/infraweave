# infraweave-mcp

Stdio MCP (Model Context Protocol) server for InfraWeave. Exposes the curated [`infraweave-tools`](../infraweave-tools) registry to IDE clients (Claude Desktop, VS Code, Cline, ...).

```
IDE -- stdio JSON-RPC --> infraweave-mcp -- HTTPS+JWT --> internal-api
```

## Configuration

Auth + endpoint are looked up in this order:

1. `INFRAWEAVE_API_ENDPOINT` + `INFRAWEAVE_TOKEN` env vars (handy for ad-hoc / CI use).
2. `~/.infraweave/tokens.json` - written by `infraweave login`. The `id_token` field is sent as a bearer to internal-api, where existing project-level authorization is the single source of truth.

Optional session defaults so the LLM doesn't have to ask on every turn:

| Var | Effect |
|---|---|
| `INFRAWEAVE_DEFAULT_PROJECT` | Default `project` for tools that need it. |
| `INFRAWEAVE_DEFAULT_REGION` | Default `region`. |
| `INFRAWEAVE_DEFAULT_ENVIRONMENT` | Default `environment`. |
| `INFRAWEAVE_DEFAULT_TRACK` | Default release `track`, e.g. `dev`. |

## Run

```bash
# CLI subcommand (preferred)
infraweave mcp

# Or the standalone binary
cargo run -p infraweave-mcp
```

## Wire it into an IDE

```bash
infraweave mcp setup-vscode      # writes servers.infraweave to VS Code's user mcp.json
infraweave mcp setup-claude      # writes mcpServers.infraweave to Claude Desktop config
```

Both commands bake the current binary path into the client configuration.

## Smoke test

```bash
echo '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"test","version":"0"}}}' | cargo run -p infraweave-mcp
```

Then `tools/list` and `tools/call` work over the same stdin pipe - see the [MCP spec](https://modelcontextprotocol.io/) for full request shapes.
