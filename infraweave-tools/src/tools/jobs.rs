use anyhow::{Context, Result};
use async_trait::async_trait;
use serde_json::{json, Value};
use std::fmt::Write;

use super::common::{opt_str, project, region, validate_project_region};
use crate::{Tool, ToolContext, ToolDef};

/// How many log lines to return. Runner logs can be enormous; the tail is where
/// the failure is, and the whole point of a curated tool is to not blow the
/// model's context on Terraform boilerplate.
const MAX_LOG_LINES: usize = 120;
/// Single lines can be whole JSON documents; cut them rather than the tail.
const MAX_LINE_CHARS: usize = 500;
/// The logs route reads forward from the head of the stream, one page per call,
/// so reaching the tail means following `nextForwardToken`. Bound it.
const MAX_LOG_PAGES: usize = 20;

pub struct GetDeploymentLogs;

#[async_trait]
impl Tool for GetDeploymentLogs {
    fn def(&self) -> ToolDef {
        ToolDef {
            name: "get_deployment_logs",
            description: "Read the raw runner logs for a job. Use this after \
                `debug_deployment` when the summarised error is not enough to explain the \
                failure - for example to see which Terraform resource actually failed, or \
                to read a provider error verbatim. The `job_id` comes from a deployment's \
                recent events.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "job_id": {
                        "type": "string",
                        "description": "Exact job id, taken from a deployment event or a job status result."
                    },
                    "project_id": {
                        "type": "string",
                        "description": "Exact InfraWeave project_id from list_projects. Do not use the project display name or account alias."
                    },
                    "region": {
                        "type": "string",
                        "description": "Exact configured cloud provider region id for the project. For AWS, use values like us-west-2 or eu-central-1; never broad aliases like us, eu, west, or production."
                    }
                },
                "required": ["job_id"]
            }),
        }
    }

    async fn execute(&self, ctx: &ToolContext, args: Value) -> Result<String> {
        let job_id = opt_str(&args, "job_id").context("`job_id` is required")?;
        let project = project(&args, ctx)?;
        let region = region(&args, ctx)?;
        validate_project_region(ctx, &project, &region).await?;

        let base = format!("/api/v1/logs/{project}/{region}/{job_id}");
        let mut text = String::new();
        let mut token: Option<String> = None;
        let mut reached_end = false;
        for _ in 0..MAX_LOG_PAGES {
            let path = match token.as_deref() {
                Some(t) => format!("{base}?next_token={}", urlencoding::encode(t)),
                None => base.clone(),
            };
            let page = ctx.api.get_json(&path).await?;
            let logs = page.get("logs").and_then(Value::as_str).unwrap_or("");
            text.push_str(logs);
            let next = page
                .get("nextForwardToken")
                .and_then(Value::as_str)
                .map(str::to_string);
            // CloudWatch signals the end by returning no events, or the same
            // forward token it was given.
            if logs.is_empty() || next.is_none() || next == token {
                reached_end = true;
                break;
            }
            token = next;
        }

        let lines: Vec<&str> = text.lines().collect();
        if lines.is_empty() {
            return Ok(format!("No logs recorded for job `{job_id}`."));
        }

        let total = lines.len();
        let mut out = format!("## Logs for job `{job_id}`\n\n");
        if total > MAX_LOG_LINES {
            let _ = writeln!(
                out,
                "_Showing the last {MAX_LOG_LINES} of {total} lines._\n"
            );
        }
        if !reached_end {
            let _ = writeln!(
                out,
                "_The log continues past the {MAX_LOG_PAGES} pages read; the job may still be running._\n"
            );
        }
        out.push_str("```\n");
        for line in &lines[total.saturating_sub(MAX_LOG_LINES)..] {
            if line.chars().count() > MAX_LINE_CHARS {
                let cut: String = line.chars().take(MAX_LINE_CHARS).collect();
                let _ = writeln!(out, "{cut} [line truncated]");
            } else {
                let _ = writeln!(out, "{line}");
            }
        }
        out.push_str("```\n");
        Ok(out)
    }
}

pub struct GetJobStatus;

#[async_trait]
impl Tool for GetJobStatus {
    fn def(&self) -> ToolDef {
        ToolDef {
            name: "get_job_status",
            description: "Check whether a job is still running, and how it finished. \
                Use this when the user asks whether something has completed yet, or to \
                confirm a run finished before reading its logs.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "job_id": { "type": "string", "description": "Exact job id." },
                    "project_id": {
                        "type": "string",
                        "description": "Exact InfraWeave project_id from list_projects. Do not use the project display name or account alias."
                    },
                    "region": {
                        "type": "string",
                        "description": "Exact configured cloud provider region id for the project. For AWS, use values like us-west-2 or eu-central-1; never broad aliases like us, eu, west, or production."
                    }
                },
                "required": ["job_id"]
            }),
        }
    }

    async fn execute(&self, ctx: &ToolContext, args: Value) -> Result<String> {
        let job_id = opt_str(&args, "job_id").context("`job_id` is required")?;
        let project = project(&args, ctx)?;
        let region = region(&args, ctx)?;
        validate_project_region(ctx, &project, &region).await?;

        let path = format!("/api/v1/job_status/{project}/{region}/{job_id}");
        let Some(status) = ctx.api.get_optional(&path).await? else {
            return Ok(format!(
                "No job `{job_id}` found in `{project}` / `{region}`."
            ));
        };

        // The job status payload varies by cloud backend, so render the fields
        // that are present rather than binding to a fixed struct.
        let mut out = format!("## Job `{job_id}`\n\n");
        if let Some(obj) = status.as_object() {
            for (key, value) in obj {
                let rendered = match value {
                    Value::String(s) => s.clone(),
                    other => other.to_string(),
                };
                if !rendered.is_empty() && rendered != "null" {
                    let _ = writeln!(out, "- **{key}:** `{rendered}`");
                }
            }
        } else {
            let _ = writeln!(out, "{status}");
        }
        Ok(out)
    }
}
