use gitops::{get_diff, group_files_by_manifest};
use serde_json::json;

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

    // Convert to JSON output for GitHub Actions
    let json_output: Vec<_> = groups
        .iter()
        .map(|group| {
            let action = if group.active.is_some() {
                "apply"
            } else if group.deleted.is_some() {
                "delete"
            } else {
                "rename"
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
                "path": file_path,
                "content": content
            })
        })
        .collect();

    // Output as compact JSON for GitHub Actions
    println!("{}", serde_json::to_string(&json_output).unwrap());
}
