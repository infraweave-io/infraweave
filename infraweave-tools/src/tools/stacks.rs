use anyhow::{Context, Result};
use async_trait::async_trait;
use env_defs::ModuleResp;
use serde_json::{json, Value};
use std::fmt::Write;

use super::archive_diff;
use super::common::{latest_by_semver, opt_str, track};
use crate::{Tool, ToolContext, ToolDef};

pub struct ListStacks;

#[async_trait]
impl Tool for ListStacks {
    fn def(&self) -> ToolDef {
        ToolDef {
            name: "list_stacks",
            description: "List the latest version of every published InfraWeave stack. \
                Stacks are bundles of modules wired together. Optionally filter by track or name substring.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "track": { "type": "string" },
                    "search": { "type": "string" }
                }
            }),
        }
    }

    async fn execute(&self, ctx: &ToolContext, args: Value) -> Result<String> {
        let stacks: Vec<ModuleResp> =
            serde_json::from_value(ctx.api.get_json("/api/v1/stacks").await?)?;
        let track_filter = opt_str(&args, "track").map(|s| s.to_string());
        let search = opt_str(&args, "search").map(|s| s.to_lowercase());
        let mut filtered: Vec<&ModuleResp> = stacks
            .iter()
            .filter(|m| {
                track_filter
                    .as_deref()
                    .map(|t| m.track == t)
                    .unwrap_or(true)
            })
            .filter(|m| {
                search
                    .as_deref()
                    .map(|q| {
                        m.module.to_lowercase().contains(q)
                            || m.module_name.to_lowercase().contains(q)
                    })
                    .unwrap_or(true)
            })
            .collect();
        filtered.sort_by(|a, b| a.module.cmp(&b.module));

        if filtered.is_empty() {
            return Ok("No stacks matched.".into());
        }
        let mut out = format!("Found {} stack(s):\n\n", filtered.len());
        for s in filtered {
            let component_count = s.stack_data.as_ref().map(|d| d.modules.len()).unwrap_or(0);
            let desc = if s.description.is_empty() {
                "(no description)"
            } else {
                s.description.as_str()
            };
            let _ = writeln!(
                out,
                "- **{}** `{}` (track: {}, version: {}, {} module(s)) - {}",
                s.module_name, s.module, s.track, s.version, component_count, desc
            );
        }
        Ok(out)
    }
}

pub struct DescribeStack;

#[async_trait]
impl Tool for DescribeStack {
    fn def(&self) -> ToolDef {
        ToolDef {
            name: "describe_stack",
            description:
                "Describe a stack: which modules it contains (with versions), inputs and outputs. \
                Use when the user asks 'what's in stack X' or 'what does stack X expose'.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "stack": { "type": "string" },
                    "track": { "type": "string" },
                    "version": { "type": "string", "description": "Omit for latest." }
                },
                "required": ["stack"]
            }),
        }
    }

    async fn execute(&self, ctx: &ToolContext, args: Value) -> Result<String> {
        let stack = opt_str(&args, "stack").context("`stack` is required")?;
        let track = track(&args, ctx)?;

        let resp: ModuleResp = if let Some(v) = opt_str(&args, "version") {
            serde_json::from_value(
                ctx.api
                    .get_json(&format!("/api/v1/stack/{track}/{stack}/{v}"))
                    .await?,
            )?
        } else {
            let versions: Vec<ModuleResp> = serde_json::from_value(
                ctx.api
                    .get_json(&format!("/api/v1/stacks/versions/{track}/{stack}"))
                    .await?,
            )?;
            latest_by_semver(versions, |s| &s.version).context("stack has no published versions")?
        };

        let mut out = format!(
            "## Stack {} `{}` v{} (track: {})\n\n{}\n\n",
            resp.module_name,
            resp.module,
            resp.version,
            resp.track,
            if resp.description.is_empty() {
                "(no description)"
            } else {
                resp.description.as_str()
            }
        );

        if let Some(stack_data) = resp.stack_data.as_ref() {
            out.push_str("### Contained modules\n");
            for m in &stack_data.modules {
                let _ = writeln!(out, "- `{}` v{} (track: {})", m.module, m.version, m.track);
            }
            out.push('\n');
        }

        if !resp.tf_variables.is_empty() {
            let (required, optional): (Vec<_>, Vec<_>) =
                resp.tf_variables.iter().partition(|v| v.required());
            let _ = writeln!(
                out,
                "### Inputs\n{} required, {} optional\n",
                required.len(),
                optional.len()
            );
            for v in required.iter().chain(optional.iter()) {
                let req = if v.required() { "required" } else { "optional" };
                let _ = writeln!(
                    out,
                    "- `{}` ({}, {}) - {}",
                    v.name,
                    v._type,
                    req,
                    if v.description.is_empty() {
                        "-"
                    } else {
                        &v.description
                    }
                );
            }
            out.push('\n');
        }

        if !resp.tf_outputs.is_empty() {
            out.push_str("### Outputs\n");
            for o in &resp.tf_outputs {
                let _ = writeln!(
                    out,
                    "- `{}` - {}",
                    o.name,
                    if o.description.is_empty() {
                        "-"
                    } else {
                        &o.description
                    }
                );
            }
        }
        Ok(out)
    }
}

pub struct DiffStackVersions;

#[async_trait]
impl Tool for DiffStackVersions {
    fn def(&self) -> ToolDef {
        ToolDef {
            name: "diff_stack_versions",
            description:
                "Show what changed between two versions of a stack: added/removed/changed \
                files by downloading both stack zips and diffing their contents. Returns \
                unified diffs that should be interpreted into a semantic summary of \
                infrastructure/code changes, not just repeated as file metadata. Use this \
                when the user asks what's different between stack versions or what would \
                change during a stack upgrade.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "stack": { "type": "string" },
                    "track": { "type": "string" },
                    "previous_version": { "type": "string", "description": "The older version to compare against." },
                    "version": { "type": "string", "description": "The newer version." }
                },
                "required": ["stack", "previous_version", "version"]
            }),
        }
    }

    async fn execute(&self, ctx: &ToolContext, args: Value) -> Result<String> {
        let stack = opt_str(&args, "stack").context("`stack` is required")?;
        let previous_version =
            opt_str(&args, "previous_version").context("`previous_version` is required")?;
        let version = opt_str(&args, "version").context("`version` is required")?;
        let track = track(&args, ctx)?;

        let old_zip = download_stack_zip(ctx, &track, stack, previous_version).await?;
        let new_zip = download_stack_zip(ctx, &track, stack, version).await?;

        archive_diff::render_zip_diff(
            "stack",
            stack,
            &track,
            previous_version,
            version,
            &old_zip,
            &new_zip,
        )
    }
}

async fn download_stack_zip(
    ctx: &ToolContext,
    track: &str,
    stack: &str,
    version: &str,
) -> Result<Vec<u8>> {
    ctx.api
        .get_bytes(&format!("/api/v1/stack/{track}/{stack}/{version}/download"))
        .await
        .with_context(|| format!("could not download stack `{stack}` {version} on track `{track}`"))
}
