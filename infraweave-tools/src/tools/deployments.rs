use anyhow::{Context, Result};
use async_trait::async_trait;
use env_defs::{DeploymentResp, EventData};
use serde_json::{json, Value};
use std::fmt::Write;

use super::common::{environment, opt_str, project, region, validate_project_region};
use crate::{Tool, ToolContext, ToolDef};

pub struct ListDeployments;

#[async_trait]
impl Tool for ListDeployments {
    fn def(&self) -> ToolDef {
        ToolDef {
            name: "list_deployments",
            description: "List deployments in a project/region. Optionally filter by module name \
                or only show deployments in a failure state. Use this when the user asks \
                'what's deployed in project X' or 'which deployments of module Y exist'.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "project_id": {
                        "type": "string",
                        "description": "Exact InfraWeave project_id from list_projects. Do not use the project display name or account alias."
                    },
                    "region": {
                        "type": "string",
                        "description": "Exact configured cloud provider region id for the project. For AWS, use values like us-west-2 or eu-central-1; never broad aliases like us, eu, west, or production."
                    },
                    "module": { "type": "string", "description": "Optional: only deployments using this module." },
                    "failures_only": { "type": "boolean", "description": "Optional: only return deployments in a failure state." }
                }
            }),
        }
    }

    async fn execute(&self, ctx: &ToolContext, args: Value) -> Result<String> {
        let project = project(&args, ctx)?;
        let region = region(&args, ctx)?;
        validate_project_region(ctx, &project, &region).await?;
        let module_filter = opt_str(&args, "module").map(|s| s.to_string());
        let failures_only = args
            .get("failures_only")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let path = match module_filter.as_ref() {
            Some(m) => format!("/api/v1/deployments/module/{project}/{region}/{m}"),
            None => format!("/api/v1/deployments/{project}/{region}"),
        };
        let deployments: Vec<DeploymentResp> =
            serde_json::from_value(ctx.api.get_json(&path).await?)?;

        let filtered: Vec<&DeploymentResp> = deployments
            .iter()
            .filter(|d| !failures_only || d.status.is_failure())
            .collect();

        if filtered.is_empty() {
            return Ok(format!(
                "No deployments matched in `{project}` / `{region}`{}.",
                if failures_only {
                    " (failures only)"
                } else {
                    ""
                }
            ));
        }

        let mut out = format!(
            "Found {} deployment(s) in `{project}` / `{region}`:\n\n",
            filtered.len()
        );
        for d in filtered {
            let _ = writeln!(
                out,
                "- `{}` (project_id: `{}`, region: `{}`, environment_id: `{}`) - module `{}` v{} - **{}**{}",
                d.deployment_id,
                d.project_id,
                d.region,
                d.environment,
                d.module,
                d.module_version,
                d.status,
                if d.has_drifted {
                    " - drift detected"
                } else {
                    ""
                }
            );
        }
        Ok(out)
    }
}

pub struct DebugDeployment;

#[async_trait]
impl Tool for DebugDeployment {
    fn def(&self) -> ToolDef {
        ToolDef {
            name: "debug_deployment",
            description: "Get a focused failure-debugging view of a deployment: current status, \
                the error text, and the most recent events. Use this whenever the user is asking \
                'why did X fail' or 'what's going on with deployment Y'.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "deployment_id": { "type": "string" },
                    "environment_id": {
                        "type": "string",
                        "description": "Exact InfraWeave environment_id from a previous deployment result. Do not use a display name or conversational alias."
                    },
                    "project_id": {
                        "type": "string",
                        "description": "Exact InfraWeave project_id from list_projects or a previous deployment result. Do not use the project display name or account alias."
                    },
                    "region": {
                        "type": "string",
                        "description": "Exact configured cloud provider region id for the project. For AWS, use values like us-west-2 or eu-central-1; never broad aliases like us, eu, west, or production."
                    }
                },
                "required": ["deployment_id", "environment_id"]
            }),
        }
    }

    async fn execute(&self, ctx: &ToolContext, args: Value) -> Result<String> {
        let deployment_id =
            opt_str(&args, "deployment_id").context("`deployment_id` is required")?;
        let environment = environment(&args, ctx)?;
        let project = project(&args, ctx)?;
        let region = region(&args, ctx)?;
        validate_project_region(ctx, &project, &region).await?;

        let dep_path =
            format!("/api/v1/deployment/{project}/{region}/{environment}/{deployment_id}");
        let Some(dep_value) = ctx.api.get_optional(&dep_path).await? else {
            return Ok(format!(
                "No deployment `{deployment_id}` found in `{project}` / `{region}` / `{environment}`."
            ));
        };
        let dep: DeploymentResp =
            serde_json::from_value(dep_value).context("could not parse deployment")?;

        let events: Vec<EventData> = ctx
            .api
            .get_json(&format!(
                "/api/v1/events/{project}/{region}/{environment}/{deployment_id}"
            ))
            .await
            .ok()
            .and_then(|v| serde_json::from_value(v).ok())
            .unwrap_or_default();

        let mut out = format!(
            "## Deployment `{}` ({} / {} / {})\n\n",
            dep.deployment_id, dep.project_id, dep.region, dep.environment
        );
        let _ = writeln!(out, "- **Status:** {}", dep.status);
        let _ = writeln!(
            out,
            "- **Module:** `{}` v{} (track: {})",
            dep.module, dep.module_version, dep.module_track
        );
        let _ = writeln!(out, "- **Initiated by:** {}", dep.initiated_by);
        if dep.has_drifted {
            let _ = writeln!(out, "- **Drift detected**");
        }
        if !dep.error_text.is_empty() {
            let _ = writeln!(out, "\n### Error\n```\n{}\n```", dep.error_text);
        }

        if !events.is_empty() {
            // Show the most recent ~6 events.
            let mut recent: Vec<&EventData> = events.iter().collect();
            recent.sort_by(|a, b| b.epoch.cmp(&a.epoch));
            recent.truncate(6);
            out.push_str("\n### Recent events\n");
            for e in recent {
                let _ = write!(out, "- `{}` - **{}**", e.timestamp, e.status);
                if !e.event.is_empty() {
                    let _ = write!(out, " ({})", e.event);
                }
                if !e.error_text.is_empty() {
                    let snippet: String = e.error_text.chars().take(200).collect();
                    let _ = write!(out, " - {snippet}");
                }
                out.push('\n');
            }
            out.push_str("\nTo dig deeper, you can request logs for the failing job_id from the most recent event.\n");
        }

        if !dep.policy_results.is_empty() {
            out.push_str("\n### Policy results\n");
            for p in &dep.policy_results {
                let _ = writeln!(out, "- {p:?}");
            }
        }

        Ok(out)
    }
}
