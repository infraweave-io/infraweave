//! InfraWeave MCP server.
//!
//! Stdio MCP server that exposes the curated `infraweave-tools` registry to
//! IDE clients (Claude Desktop, VS Code, Cline, ...).
//!
//! Architecture (refactored from the OpenAPI auto-generation it used to do):
//!
//! ```text
//! IDE -- stdio JSON-RPC --> infraweave-mcp -- HTTPS+JWT --> internal-api
//! ```

use anyhow::{anyhow, Context, Result};
use infraweave_tools::{registry, ApiClient, Tool, ToolContext};
use rmcp::{
    handler::server::ServerHandler,
    model::{
        CallToolRequestParams, CallToolResult, Content, Implementation, InitializeRequestParams,
        InitializeResult, ListToolsResult, PaginatedRequestParams, ProtocolVersion,
        ServerCapabilities, ServerInfo, Tool as McpTool, ToolAnnotations, ToolsCapability,
    },
    service::{RequestContext, RoleServer},
    ErrorData as McpError, ServiceExt,
};
use serde_json::Value;

/// Run the MCP server on stdio. Blocks until the client disconnects.
pub async fn run() -> Result<()> {
    eprintln!("=== InfraWeave MCP Server ===");

    let (endpoint, token) = load_endpoint_and_token()?;
    eprintln!("[MCP] api endpoint: {endpoint}");

    let api = ApiClient::new(endpoint, token).context("could not build API client")?;

    let mut tool_ctx = ToolContext::new(api);
    if let Ok(p) = std::env::var("INFRAWEAVE_DEFAULT_PROJECT") {
        tool_ctx = tool_ctx.with_project(p);
    }
    if let Ok(r) = std::env::var("INFRAWEAVE_DEFAULT_REGION") {
        tool_ctx = tool_ctx.with_region(r);
    }
    if let Ok(e) = std::env::var("INFRAWEAVE_DEFAULT_ENVIRONMENT") {
        tool_ctx = tool_ctx.with_environment(e);
    }
    if let Ok(t) = std::env::var("INFRAWEAVE_DEFAULT_TRACK") {
        tool_ctx = tool_ctx.with_track(t);
    }

    let tools = registry();
    eprintln!("[MCP] {} tools registered", tools.len());

    let handler = InfraWeaveServer { tool_ctx, tools };

    let transport = (tokio::io::stdin(), tokio::io::stdout());
    eprintln!("=== Server Ready ===");

    let running = handler
        .serve(transport)
        .await
        .map_err(|e| anyhow!("MCP server failed to start: {e}"))?;
    running
        .waiting()
        .await
        .map_err(|e| anyhow!("MCP server exited with error: {e}"))?;
    Ok(())
}

/// Read the InfraWeave API endpoint + JWT from the same locations the CLI uses.
///
/// Order:
///   1. `INFRAWEAVE_API_ENDPOINT` + `INFRAWEAVE_TOKEN` env vars (for ad-hoc / CI use).
///   2. `~/.infraweave/tokens.json` (written by `infraweave login`).
///
/// The token is sent as a bearer to internal-api; project-level authorization
/// there is the single source of truth, so MCP doesn't validate it.
fn load_endpoint_and_token() -> Result<(String, String)> {
    if let (Ok(endpoint), Ok(token)) = (
        std::env::var("INFRAWEAVE_API_ENDPOINT"),
        std::env::var("INFRAWEAVE_TOKEN"),
    ) {
        return Ok((endpoint, token));
    }

    let path = env_utils::config_path::get_token_path()
        .context("could not resolve infraweave config path")?;
    if !path.exists() {
        return Err(anyhow!(
            "no auth configured: run `infraweave login --api-endpoint <url>`, or set \
             INFRAWEAVE_API_ENDPOINT + INFRAWEAVE_TOKEN env vars"
        ));
    }
    let json = std::fs::read_to_string(&path)
        .with_context(|| format!("failed to read {}", path.display()))?;
    let v: Value = serde_json::from_str(&json).context("tokens.json is not valid JSON")?;

    let endpoint = std::env::var("INFRAWEAVE_API_ENDPOINT")
        .ok()
        .or_else(|| {
            v.get("api_endpoint")
                .and_then(|s| s.as_str())
                .map(String::from)
        })
        .ok_or_else(|| anyhow!("no api_endpoint in tokens.json"))?;
    let token = v
        .get("id_token")
        .and_then(|s| s.as_str())
        .map(String::from)
        .ok_or_else(|| anyhow!("no id_token in tokens.json - re-run `infraweave login`"))?;
    Ok((endpoint, token))
}

struct InfraWeaveServer {
    tool_ctx: ToolContext,
    tools: Vec<Box<dyn Tool>>,
}

impl ServerHandler for InfraWeaveServer {
    fn get_info(&self) -> ServerInfo {
        let mut capabilities = ServerCapabilities::default();
        capabilities.tools = Some(ToolsCapability {
            list_changed: Some(false),
        });

        InitializeResult::new(capabilities)
            .with_protocol_version(ProtocolVersion::default())
            .with_server_info(
                Implementation::new("infraweave-mcp", env!("CARGO_PKG_VERSION"))
                    .with_title("InfraWeave"),
            )
            .with_instructions(
                "Curated tools for inspecting InfraWeave modules, stacks, deployments, and \
                 debugging deployment failures. Pass `project`, `region`, `environment`, or \
                 `track` arguments where required.",
            )
    }

    async fn initialize(
        &self,
        request: InitializeRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<InitializeResult, McpError> {
        if context.peer.peer_info().is_none() {
            context.peer.set_peer_info(request);
        }
        Ok(self.get_info())
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, McpError> {
        let tools = self
            .tools
            .iter()
            .map(|t| {
                let def = t.def();
                // ToolDef.input_schema is a JSON object (`{"type":"object", ...}`).
                // rmcp wants it as a JsonObject (serde_json::Map<String, Value>).
                let schema = def.input_schema.as_object().cloned().unwrap_or_default();
                McpTool::new(def.name, def.description, schema).with_annotations(
                    // All current tools are read-only inspections - flag that so
                    // clients can surface them without confirmation prompts.
                    ToolAnnotations::new().read_only(true),
                )
            })
            .collect();
        Ok(ListToolsResult::with_all_items(tools))
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        let Some(tool) = self
            .tools
            .iter()
            .find(|t| t.def().name == request.name.as_ref())
        else {
            return Err(McpError::invalid_params(
                format!("unknown tool `{}`", request.name),
                None,
            ));
        };

        let args = request
            .arguments
            .map(Value::Object)
            .unwrap_or(Value::Object(Default::default()));

        match tool.execute(&self.tool_ctx, args).await {
            Ok(text) => Ok(CallToolResult::success(vec![Content::text(text)])),
            Err(e) => Ok(CallToolResult::error(vec![Content::text(format!("{e:#}"))])),
        }
    }
}
