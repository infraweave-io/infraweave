use gitops::{get_diff, group_files_by_manifest};
use serde_json::json;
use std::process::Command;

fn get_current_branch() -> Result<String, String> {
    let output = Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .output()
        .map_err(|e| format!("Failed to get current branch: {}", e))?;

    if !output.status.success() {
        return Err("Failed to get current branch".to_string());
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn get_default_branch() -> Result<String, String> {
    let output = Command::new("git")
        .args(["symbolic-ref", "refs/remotes/origin/HEAD", "--short"])
        .output()
        .map_err(|e| format!("Failed to get default branch: {}", e))?;

    if !output.status.success() {
        // Fallback to "origin/main"
        return Ok("origin/main".to_string());
    }

    let branch = String::from_utf8_lossy(&output.stdout).trim().to_string();
    // Remove "origin/" prefix if present
    Ok(branch.trim_start_matches("origin/").to_string())
}

pub async fn handle_diff(before: &str, after: &str) {
    // Get the diff between the two git references
    let processed = match get_diff(before, after) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("Error getting diff: {}", e);
            std::process::exit(1);
        }
    };

    // Group files by manifest
    let groups = group_files_by_manifest(processed);

    // Priority: CURRENT_BRANCH env var (for CI/CD systems) > git command
    let current_branch = std::env::var("CURRENT_BRANCH")
        .ok()
        .or_else(|| get_current_branch().ok())
        .unwrap_or_else(|| "unknown".to_string());

    // Priority: DEFAULT_BRANCH env var (for CI/CD systems) > git command
    let default_branch = std::env::var("DEFAULT_BRANCH")
        .ok()
        .or_else(|| get_default_branch().ok())
        .unwrap_or_else(|| "main".to_string());

    let is_default_branch = current_branch == default_branch;

    // Convert to JSON output for GitHub Actions
    let json_output: Vec<_> = groups
        .iter()
        .map(|group| {
            let (action, flags) = if group.active.is_some() {
                if is_default_branch {
                    ("apply", vec![])
                } else {
                    ("plan", vec![])
                }
            } else if group.deleted.is_some() {
                if is_default_branch {
                    ("destroy", vec![])
                } else {
                    ("plan", vec!["--destroy".to_string()])
                }
            } else {
                ("rename", vec![])
            };

            let file_path = group
                .active
                .as_ref()
                .or(group.deleted.as_ref())
                .or(group.renamed.as_ref())
                .map(|(f, _)| f.path.clone())
                .unwrap_or_default();

            let content = if let Some((_, yaml)) = &group.active {
                Some(yaml.clone())
            } else if let Some((_, yaml)) = &group.deleted {
                Some(yaml.clone())
            } else if let Some((_, yaml)) = &group.renamed {
                Some(yaml.clone())
            } else {
                None
            };

            json!({
                "name": group.key.name,
                "kind": group.key.kind,
                "namespace": group.key.namespace,
                "region": group.key.region,
                "apiVersion": group.key.api_version,
                "action": action,
                "flags": flags,
                "path": file_path,
                "content": content
            })
        })
        .collect();

    // Output as compact JSON for GitHub Actions
    println!("{}", serde_json::to_string(&json_output).unwrap());
}
