use anyhow::Result;
use async_trait::async_trait;
use env_defs::ProjectData;
use serde_json::{json, Value};
use std::fmt::Write;

use crate::{Tool, ToolContext, ToolDef};

pub struct ListProjects;

#[async_trait]
impl Tool for ListProjects {
    fn def(&self) -> ToolDef {
        ToolDef {
            name: "list_projects",
            description: "List all InfraWeave projects the caller has access to. \
                Use this when the user asks 'what projects do I have' or needs to disambiguate \
                a project before another tool can run.",
            input_schema: json!({ "type": "object", "properties": {} }),
        }
    }

    async fn execute(&self, ctx: &ToolContext, _args: Value) -> Result<String> {
        let projects: Vec<ProjectData> =
            serde_json::from_value(ctx.api.get_json("/api/v1/projects").await?)?;
        if projects.is_empty() {
            return Ok("No projects visible.".into());
        }
        let mut out = format!("Found {} project(s):\n\n", projects.len());
        for p in &projects {
            let regions = if p.regions.is_empty() {
                "-".to_string()
            } else {
                p.regions.join(", ")
            };
            let desc = if p.description.is_empty() {
                "(no description)"
            } else {
                p.description.as_str()
            };
            let _ = writeln!(
                out,
                "- **{}** `{}` (regions: {}) - {}",
                p.name, p.project_id, regions, desc
            );
        }
        Ok(out)
    }
}
