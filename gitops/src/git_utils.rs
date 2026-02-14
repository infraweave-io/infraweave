use std::process::Command;

/// Get file content at a specific git reference (commit SHA, branch, or tag)
pub fn get_file_content(file_path: &str, git_ref: &str) -> Result<String, String> {
    let output = Command::new("git")
        .args(["show", &format!("{}:{}", git_ref, file_path)])
        .output()
        .map_err(|e| format!("Failed to run git command: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("Git command failed: {}", stderr));
    }

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

/// Get list of changed files between two git references
/// Returns a list of (status, file_path) tuples where status is one of: A (added), M (modified), D (deleted)
pub fn get_changed_files(
    before_ref: &str,
    after_ref: &str,
) -> Result<Vec<(String, String)>, String> {
    let output = Command::new("git")
        .args(["diff", "--name-status", before_ref, after_ref])
        .output()
        .map_err(|e| format!("Failed to run git diff: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("Git diff failed: {}", stderr));
    }

    let diff_output = String::from_utf8_lossy(&output.stdout);
    let mut changes = Vec::new();

    for line in diff_output.lines() {
        let parts: Vec<&str> = line.split('\t').collect();
        if parts.len() < 2 {
            continue;
        }

        let status = parts[0].to_string();
        let file_path = parts[1].to_string();

        // Only include YAML files
        if file_path.ends_with(".yaml") || file_path.ends_with(".yml") {
            changes.push((status, file_path));
        }
    }

    Ok(changes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_git_diff_output() {
        // Test parsing of git diff output
        let diff_output = "A\tclaims/new-bucket.yaml\nM\tclaims/existing-bucket.yaml\nD\tclaims/old-bucket.yaml\nM\tREADME.md\n";

        let mut changes = Vec::new();
        for line in diff_output.lines() {
            let parts: Vec<&str> = line.split('\t').collect();
            if parts.len() < 2 {
                continue;
            }
            let status = parts[0].to_string();
            let file_path = parts[1].to_string();

            // Only include YAML files
            if file_path.ends_with(".yaml") || file_path.ends_with(".yml") {
                changes.push((status, file_path));
            }
        }

        assert_eq!(changes.len(), 3);
        assert_eq!(
            changes[0],
            ("A".to_string(), "claims/new-bucket.yaml".to_string())
        );
        assert_eq!(
            changes[1],
            ("M".to_string(), "claims/existing-bucket.yaml".to_string())
        );
        assert_eq!(
            changes[2],
            ("D".to_string(), "claims/old-bucket.yaml".to_string())
        );
    }

    #[test]
    fn test_yaml_file_filter() {
        let files = [
            "claims/bucket.yaml",
            "claims/bucket.yml",
            "README.md",
            "claims/test.txt",
        ];

        let yaml_files: Vec<&str> = files
            .iter()
            .filter(|f| f.ends_with(".yaml") || f.ends_with(".yml"))
            .copied()
            .collect();

        assert_eq!(yaml_files.len(), 2);
        assert!(yaml_files.contains(&"claims/bucket.yaml"));
        assert!(yaml_files.contains(&"claims/bucket.yml"));
    }

    #[test]
    #[ignore] // Requires git repository with commits
    fn test_get_file_content_integration() {
        // This test requires an actual git repo
        // Run with: cargo test -- --ignored
        let result = get_file_content("Cargo.toml", "HEAD");
        assert!(result.is_ok());
    }

    #[test]
    #[ignore] // Requires git repository with commits
    fn test_get_changed_files_integration() {
        // This test requires an actual git repo
        // Run with: cargo test -- --ignored
        let result = get_changed_files("HEAD~1", "HEAD");
        assert!(result.is_ok());
    }
}
