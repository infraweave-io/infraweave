use anyhow::{anyhow, Result};
use env_defs::ProjectData;
use semver::Version;
use serde_json::Value;
use std::cmp::Ordering;

use crate::ToolContext;

/// Read a string field from the tool args, falling back to a context default.
pub fn arg_or_default(
    args: &Value,
    field: &str,
    default: Option<&String>,
    field_pretty: &str,
) -> Result<String> {
    if let Some(v) = args
        .get(field)
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
    {
        return Ok(v.to_string());
    }
    if let Some(d) = default {
        return Ok(d.clone());
    }
    Err(anyhow!(
        "missing required argument `{field}` ({field_pretty}) and no session default is set"
    ))
}

pub fn opt_str<'a>(args: &'a Value, field: &str) -> Option<&'a str> {
    args.get(field)
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
}

pub fn project(args: &Value, ctx: &ToolContext) -> Result<String> {
    arg_or_default(
        args,
        "project_id",
        ctx.default_project.as_ref(),
        "exact InfraWeave project_id",
    )
    .or_else(|_| arg_or_default(args, "project", ctx.default_project.as_ref(), "project id"))
}

pub fn region(args: &Value, ctx: &ToolContext) -> Result<String> {
    arg_or_default(
        args,
        "region",
        ctx.default_region.as_ref(),
        "exact cloud provider region id (for AWS, use values like us-west-2 or eu-central-1)",
    )
}

pub async fn validate_project_region(
    ctx: &ToolContext,
    project_id: &str,
    region: &str,
) -> Result<()> {
    let projects: Vec<ProjectData> =
        serde_json::from_value(ctx.api.get_json("/api/v1/projects").await?)?;
    let Some(project) = projects.iter().find(|p| p.project_id == project_id) else {
        return Err(anyhow!(
            "unknown project_id `{project_id}`. Call `list_projects` and use the exact project_id."
        ));
    };
    if project.regions.iter().any(|r| r == region) {
        return Ok(());
    }
    let regions = if project.regions.is_empty() {
        "no configured regions".to_string()
    } else {
        project.regions.join(", ")
    };
    Err(anyhow!(
        "invalid region `{region}` for project_id `{project_id}`. Use one of the exact configured cloud provider region ids: {regions}"
    ))
}

pub fn environment(args: &Value, ctx: &ToolContext) -> Result<String> {
    arg_or_default(
        args,
        "environment_id",
        ctx.default_environment.as_ref(),
        "exact InfraWeave environment_id",
    )
    .or_else(|_| {
        arg_or_default(
            args,
            "environment",
            ctx.default_environment.as_ref(),
            "environment id",
        )
    })
}

pub fn track(args: &Value, ctx: &ToolContext) -> Result<String> {
    arg_or_default(
        args,
        "track",
        ctx.default_track.as_ref(),
        "release track (e.g. dev, stable)",
    )
}

pub fn latest_by_semver<T, F>(items: Vec<T>, version: F) -> Option<T>
where
    F: Fn(&T) -> &str,
{
    items
        .into_iter()
        .max_by(|a, b| compare_versions(version(a), version(b)))
}

fn compare_versions(a: &str, b: &str) -> Ordering {
    match (Version::parse(a), Version::parse(b)) {
        (Ok(a_semver), Ok(b_semver)) => a_semver.cmp(&b_semver).then_with(|| a.cmp(b)),
        (Ok(_), Err(_)) => Ordering::Greater,
        (Err(_), Ok(_)) => Ordering::Less,
        (Err(_), Err(_)) => a.cmp(b),
    }
}

#[cfg(test)]
mod tests {
    use super::latest_by_semver;

    #[test]
    fn latest_by_semver_handles_multi_digit_components() {
        let versions = vec!["1.9.0", "1.10.0", "1.2.0"];

        let latest = latest_by_semver(versions, |v| v).unwrap();

        assert_eq!(latest, "1.10.0");
    }

    #[test]
    fn latest_by_semver_prefers_valid_semver_over_invalid_versions() {
        let versions = vec!["banana", "1.0.0"];

        let latest = latest_by_semver(versions, |v| v).unwrap();

        assert_eq!(latest, "1.0.0");
    }
}
