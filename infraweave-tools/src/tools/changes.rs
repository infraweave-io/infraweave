use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use env_defs::InfraChangeRecord;
use serde_json::{json, Value};
use std::fmt::Write;

use super::common::{environment, opt_str, project, region, validate_project_region};
use crate::{Tool, ToolContext, ToolDef};

const MAX_RECORDS: usize = 10;
const MAX_PLAN_CHARS: usize = 4000;
const MAX_RESOURCE_CHANGES: usize = 50;
const MAX_CHANGED_ATTRIBUTES: usize = 8;

/// The change-record routes key records by `mutate` (apply and destroy, plus
/// legacy records) or `plan`, and require one of them on every request.
const ALL_TYPES: [&str; 2] = ["mutate", "plan"];

/// Map the model-facing `change_type` onto the route's record types. No filter
/// means both, because the model asking "what changed" wants plans and applies.
fn record_types(args: &Value) -> Result<Vec<&'static str>> {
    let Some(raw) = opt_str(args, "change_type") else {
        return Ok(ALL_TYPES.to_vec());
    };
    match raw.to_lowercase().as_str() {
        "plan" => Ok(vec!["plan"]),
        "apply" | "destroy" | "mutate" => Ok(vec!["mutate"]),
        _ => Err(anyhow!(
            "invalid `change_type` `{raw}`. Use `plan` or `apply`, or omit it for both."
        )),
    }
}

/// The most informative line of a Terraform run, for one-line summaries.
fn summary_line(output: &str) -> &str {
    output
        .lines()
        .map(str::trim)
        .find(|l| {
            l.contains("to add") || l.starts_with("Apply complete") || l.starts_with("No changes")
        })
        .or_else(|| output.lines().map(str::trim).find(|l| !l.is_empty()))
        .unwrap_or("")
}

pub struct ListChangeRecords;

#[async_trait]
impl Tool for ListChangeRecords {
    fn def(&self) -> ToolDef {
        ToolDef {
            name: "list_change_records",
            description: "List recent change records for a deployment - every plan and \
                apply, newest first, with the module version each ran against. Use this \
                whenever the user asks what changed, when something last worked, or why \
                behaviour differs from before. Pair it with `describe_change_record` to \
                read a specific change in detail.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "deployment_id": { "type": "string", "description": "Exact deployment_id." },
                    "environment_id": {
                        "type": "string",
                        "description": "Exact InfraWeave environment_id from a previous deployment result. Do not use a display name or conversational alias."
                    },
                    "project_id": {
                        "type": "string",
                        "description": "Exact InfraWeave project_id from list_projects. Do not use the project display name or account alias."
                    },
                    "region": {
                        "type": "string",
                        "description": "Exact configured cloud provider region id for the project. For AWS, use values like us-west-2 or eu-central-1; never broad aliases like us, eu, west, or production."
                    },
                    "change_type": {
                        "type": "string",
                        "description": "Optional filter: `plan` or `apply`. Omit to list both."
                    }
                },
                "required": ["deployment_id"]
            }),
        }
    }

    async fn execute(&self, ctx: &ToolContext, args: Value) -> Result<String> {
        let deployment_id =
            opt_str(&args, "deployment_id").context("`deployment_id` is required")?;
        let types = record_types(&args)?;
        let environment = environment(&args, ctx)?;
        let project = project(&args, ctx)?;
        let region = region(&args, ctx)?;
        validate_project_region(ctx, &project, &region).await?;

        let mut records: Vec<InfraChangeRecord> = Vec::new();
        for record_type in types {
            let path = format!(
                "/api/v1/change_records/{project}/{region}/{environment}/{deployment_id}?change_type={record_type}&limit={MAX_RECORDS}"
            );
            let page: Vec<InfraChangeRecord> =
                serde_json::from_value(ctx.api.get_json(&path).await?)
                    .context("could not parse change records")?;
            records.extend(page);
        }
        records.sort_by(|a, b| b.epoch.cmp(&a.epoch));

        if records.is_empty() {
            return Ok(format!(
                "No change records for `{deployment_id}` in `{project}` / `{region}` / `{environment}`."
            ));
        }

        let total = records.len();
        records.truncate(MAX_RECORDS);

        let mut out = format!("## Change records for `{deployment_id}`\n\n");
        if total > MAX_RECORDS {
            let _ = writeln!(
                out,
                "_Showing the {MAX_RECORDS} most recent of {total} fetched._\n"
            );
        }
        for r in &records {
            let _ = writeln!(
                out,
                "- `{}` - **{}** of `{}` v{} (job_id: `{}`)",
                r.timestamp, r.change_type, r.module, r.module_version, r.job_id
            );
            let summary = summary_line(&r.plan_std_output);
            if !summary.is_empty() {
                let _ = writeln!(out, "  - {summary}");
            }
        }
        let _ = writeln!(
            out,
            "\nUse `describe_change_record` with a `job_id` above to see the full plan."
        );
        Ok(out)
    }
}

pub struct DescribeChangeRecord;

#[async_trait]
impl Tool for DescribeChangeRecord {
    fn def(&self) -> ToolDef {
        ToolDef {
            name: "describe_change_record",
            description: "Read one change record in full: the plan output, the resource \
                changes it made, and the names of the variables it ran with. Use this \
                after `list_change_records` to explain exactly what a specific plan or \
                apply did.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "deployment_id": { "type": "string", "description": "Exact deployment_id." },
                    "job_id": {
                        "type": "string",
                        "description": "Exact job_id from a list_change_records result."
                    },
                    "change_type": {
                        "type": "string",
                        "description": "Optional: `plan` or `apply`, as shown in the list_change_records result. Omit it to look for an apply first, then a plan."
                    },
                    "environment_id": {
                        "type": "string",
                        "description": "Exact InfraWeave environment_id from a previous deployment result. Do not use a display name or conversational alias."
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
                "required": ["deployment_id", "job_id"]
            }),
        }
    }

    async fn execute(&self, ctx: &ToolContext, args: Value) -> Result<String> {
        let deployment_id =
            opt_str(&args, "deployment_id").context("`deployment_id` is required")?;
        let job_id = opt_str(&args, "job_id").context("`job_id` is required")?;
        let types = record_types(&args)?;
        let environment = environment(&args, ctx)?;
        let project = project(&args, ctx)?;
        let region = region(&args, ctx)?;
        validate_project_region(ctx, &project, &region).await?;

        let mut found = None;
        for record_type in types {
            let path = format!(
                "/api/v1/change_record/{project}/{region}/{environment}/{deployment_id}/{job_id}/{record_type}"
            );
            if let Some(value) = ctx.api.get_optional(&path).await? {
                found = Some(value);
                break;
            }
        }
        let Some(value) = found else {
            return Ok(format!(
                "No change record `{job_id}` for deployment `{deployment_id}`."
            ));
        };
        let record: InfraChangeRecord =
            serde_json::from_value(value).context("could not parse change record")?;

        let mut out = format!(
            "## {} of `{}` v{}\n\n",
            record.change_type, record.module, record.module_version
        );
        let _ = writeln!(out, "- **When:** {}", record.timestamp);
        let _ = writeln!(out, "- **Job:** `{}`", record.job_id);
        let _ = writeln!(
            out,
            "- **Where:** `{}` / `{}` / `{}`",
            record.project_id, record.region, record.environment
        );

        if !record.resource_changes.is_empty() {
            let _ = writeln!(
                out,
                "\n### Resource changes ({})\n",
                record.resource_changes.len()
            );
            for change in record.resource_changes.iter().take(MAX_RESOURCE_CHANGES) {
                let action = serde_json::to_value(&change.action)
                    .ok()
                    .and_then(|v| v.as_str().map(str::to_string))
                    .unwrap_or_else(|| format!("{:?}", change.action));
                let _ = write!(out, "- `{}` - **{action}**", change.address);
                if let Some(reason) = change.action_reason.as_deref() {
                    let _ = write!(out, " ({reason})");
                }
                // Attribute paths only: `before` / `after` hold values, which can
                // be secrets, and the model does not need them to explain a change.
                if let Some(changes) = &change.changes {
                    let paths: Vec<&str> = changes
                        .keys()
                        .take(MAX_CHANGED_ATTRIBUTES)
                        .map(String::as_str)
                        .collect();
                    if !paths.is_empty() {
                        let _ = write!(out, " - changed: `{}`", paths.join("`, `"));
                    }
                }
                out.push('\n');
            }
            if record.resource_changes.len() > MAX_RESOURCE_CHANGES {
                let _ = writeln!(
                    out,
                    "_Showing the first {MAX_RESOURCE_CHANGES} of {}._",
                    record.resource_changes.len()
                );
            }
        }

        if let Some(variables) = record.variables.as_object().filter(|v| !v.is_empty()) {
            let names: Vec<&str> = variables.keys().map(String::as_str).collect();
            let _ = writeln!(
                out,
                "\n### Variables (names only; values are withheld)\n\n- `{}`",
                names.join("`, `")
            );
        }

        if !record.plan_std_output.trim().is_empty() {
            let plan: String = record
                .plan_std_output
                .chars()
                .take(MAX_PLAN_CHARS)
                .collect();
            let _ = writeln!(out, "\n### Plan output\n```\n{plan}\n```");
            if record.plan_std_output.chars().count() > MAX_PLAN_CHARS {
                let _ = writeln!(out, "_Plan output truncated._");
            }
        }
        Ok(out)
    }
}
