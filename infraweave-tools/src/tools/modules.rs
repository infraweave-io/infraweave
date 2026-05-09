use anyhow::{Context, Result};
use async_trait::async_trait;
use env_defs::ModuleResp;
use serde_json::{json, Value};
use std::fmt::Write;

use super::archive_diff;
use super::common::{latest_by_semver, opt_str, track};
use crate::{Tool, ToolContext, ToolDef};

pub struct ListModules;

#[async_trait]
impl Tool for ListModules {
    fn def(&self) -> ToolDef {
        ToolDef {
            name: "list_modules",
            description: "List the latest version of every published InfraWeave module. \
                Optionally filter by track (e.g. 'dev', 'stable') or substring search on the module name. \
                Returns name, latest version, and short description for each.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "track": { "type": "string", "description": "Optional release track to filter by (e.g. 'dev', 'stable')." },
                    "search": { "type": "string", "description": "Optional case-insensitive substring to filter module names by." }
                }
            }),
        }
    }

    async fn execute(&self, ctx: &ToolContext, args: Value) -> Result<String> {
        let modules: Vec<ModuleResp> =
            serde_json::from_value(ctx.api.get_json("/api/v1/modules").await?)
                .context("could not parse modules list")?;

        let track_filter = opt_str(&args, "track").map(|s| s.to_string());
        let search = opt_str(&args, "search").map(|s| s.to_lowercase());

        let mut filtered: Vec<&ModuleResp> = modules
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
            return Ok("No modules matched.".into());
        }

        let mut out = format!("Found {} module(s):\n\n", filtered.len());
        for m in filtered {
            let desc = if m.description.is_empty() {
                "(no description)"
            } else {
                m.description.as_str()
            };
            let _ = writeln!(
                out,
                "- **{}** `{}` (track: {}, version: {}) - {}",
                m.module_name, m.module, m.track, m.version, desc
            );
        }
        Ok(out)
    }
}

pub struct DescribeModule;

#[async_trait]
impl Tool for DescribeModule {
    fn def(&self) -> ToolDef {
        ToolDef {
            name: "describe_module",
            description: "Describe a specific module version: required and optional inputs, outputs, \
                and required Terraform providers. Use this when the user asks 'what does module X take' \
                or 'what does it output'. If `version` is omitted, the latest version on the track is used.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "module": { "type": "string", "description": "Module name (required)." },
                    "track": { "type": "string", "description": "Release track. Falls back to session default." },
                    "version": { "type": "string", "description": "Optional specific version; omit for latest." }
                },
                "required": ["module"]
            }),
        }
    }

    async fn execute(&self, ctx: &ToolContext, args: Value) -> Result<String> {
        let module = opt_str(&args, "module")
            .context("`module` is required")?
            .to_string();
        let track = track(&args, ctx)?;

        let module_resp: ModuleResp = if let Some(v) = opt_str(&args, "version") {
            serde_json::from_value(
                ctx.api
                    .get_json(&format!("/api/v1/module/{track}/{module}/{v}"))
                    .await?,
            )?
        } else {
            let versions: Vec<ModuleResp> = serde_json::from_value(
                ctx.api
                    .get_json(&format!("/api/v1/modules/versions/{track}/{module}"))
                    .await?,
            )?;
            latest_by_semver(versions, |m| &m.version)
                .context("module has no published versions")?
        };

        Ok(render_module(&module_resp))
    }
}

pub struct DiffModuleVersions;

#[async_trait]
impl Tool for DiffModuleVersions {
    fn def(&self) -> ToolDef {
        ToolDef {
            name: "diff_module_versions",
            description:
                "Show what changed between two versions of a module: added/removed/changed \
                files by downloading both module zips and diffing their contents. Returns \
                unified diffs that should be interpreted into a semantic summary of \
                infrastructure/code changes, not just repeated as file metadata. Use this \
                when the user asks 'what's different in v1.3 vs v1.2' or 'what would I \
                have to change to upgrade'.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "module": { "type": "string" },
                    "track": { "type": "string" },
                    "version": { "type": "string", "description": "The newer version." },
                    "previous_version": { "type": "string", "description": "The older version to compare against." }
                },
                "required": ["module", "previous_version", "version"]
            }),
        }
    }

    async fn execute(&self, ctx: &ToolContext, args: Value) -> Result<String> {
        let module = opt_str(&args, "module").context("`module` is required")?;
        let previous_version =
            opt_str(&args, "previous_version").context("`previous_version` is required")?;
        let version = opt_str(&args, "version").context("`version` is required")?;
        let track = track(&args, ctx)?;

        let old_zip = download_module_zip(ctx, &track, module, previous_version).await?;
        let new_zip = download_module_zip(ctx, &track, module, version).await?;

        archive_diff::render_zip_diff(
            "module",
            module,
            &track,
            previous_version,
            version,
            &old_zip,
            &new_zip,
        )
    }
}

fn render_module(m: &ModuleResp) -> String {
    let mut out = format!(
        "## {} `{}` v{} (track: {})\n\n{}\n\n",
        m.module_name,
        m.module,
        m.version,
        m.track,
        if m.description.is_empty() {
            "(no description)"
        } else {
            m.description.as_str()
        }
    );

    let (required, optional): (Vec<_>, Vec<_>) = m.tf_variables.iter().partition(|v| v.required());
    if !required.is_empty() {
        out.push_str("### Required inputs\n");
        for v in &required {
            let _ = writeln!(
                out,
                "- `{}` ({}) - {}",
                v.name,
                v._type,
                default_or_dash(&v.description)
            );
        }
        out.push('\n');
    }
    if !optional.is_empty() {
        out.push_str("### Optional inputs\n");
        for v in &optional {
            let default = v
                .default
                .as_ref()
                .map(|d| serde_json::to_string(d).unwrap_or_default())
                .unwrap_or_else(|| "-".into());
            let _ = writeln!(
                out,
                "- `{}` ({}, default: `{}`) - {}",
                v.name,
                v._type,
                default,
                default_or_dash(&v.description)
            );
        }
        out.push('\n');
    }

    if !m.tf_outputs.is_empty() {
        out.push_str("### Outputs\n");
        for o in &m.tf_outputs {
            let _ = writeln!(out, "- `{}` - {}", o.name, default_or_dash(&o.description));
        }
        out.push('\n');
    }

    if !m.tf_required_providers.is_empty() {
        out.push_str("### Required providers\n");
        for p in &m.tf_required_providers {
            let _ = writeln!(
                out,
                "- `{}` (`{}`) version `{}`",
                p.name, p.source, p.version
            );
        }
    }
    out
}

async fn download_module_zip(
    ctx: &ToolContext,
    track: &str,
    module: &str,
    version: &str,
) -> Result<Vec<u8>> {
    ctx.api
        .get_bytes(&format!(
            "/api/v1/module/{track}/{module}/{version}/download"
        ))
        .await
        .with_context(|| format!("could not download `{module}` {version} on track `{track}`"))
}

fn default_or_dash(s: &str) -> &str {
    if s.trim().is_empty() {
        "-"
    } else {
        s
    }
}
