use crate::defs::{FileChange, ProcessedFiles};
use crate::git_utils::{get_changed_files, get_file_content};
use std::env;

/// Detect changed YAML files between two git references and build ProcessedFiles
pub fn get_diff(before_ref: &str, after_ref: &str) -> Result<ProcessedFiles, String> {
    let changes = get_changed_files(before_ref, after_ref)?;

    // Apply path prefix filter if configured
    let path_prefix = env::var("FILE_PATH_PREFIX").ok();
    let filtered_changes: Vec<(String, String)> = if let Some(ref prefix_env) = path_prefix {
        let prefixes: Vec<&str> = prefix_env.split(',').map(|s| s.trim()).collect();
        println!("Filtering files with prefixes: {:?}", prefixes);

        changes
            .into_iter()
            .filter(|(_, file_path)| prefixes.iter().any(|prefix| file_path.starts_with(prefix)))
            .collect()
    } else {
        changes
    };

    let mut active_files = Vec::new();
    let mut deleted_files = Vec::new();

    for (status, file_path) in filtered_changes {
        match status.as_str() {
            "A" | "M" => {
                // Added or modified files - get content at after_ref
                match get_file_content(&file_path, after_ref) {
                    Ok(content) => {
                        active_files.push(FileChange {
                            path: file_path.clone(),
                            content,
                        });

                        // For modified files, also get the before content
                        if status == "M" {
                            if let Ok(before_content) = get_file_content(&file_path, before_ref) {
                                deleted_files.push(FileChange {
                                    path: file_path,
                                    content: before_content,
                                });
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!("Warning: Could not get content for {}: {}", file_path, e);
                    }
                }
            }
            "D" => {
                // Deleted files - get content at before_ref
                match get_file_content(&file_path, before_ref) {
                    Ok(content) => {
                        deleted_files.push(FileChange {
                            path: file_path,
                            content,
                        });
                    }
                    Err(e) => {
                        eprintln!("Warning: Could not get content for {}: {}", file_path, e);
                    }
                }
            }
            _ => {
                // Ignore other statuses (R for renamed, etc.)
                eprintln!("Ignoring file with status {}: {}", status, file_path);
            }
        }
    }

    Ok(ProcessedFiles {
        active_files,
        deleted_files,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::defs::{FileChange, ProcessedFiles};

    #[test]
    fn test_path_prefix_filtering() {
        // Test the path filtering logic in isolation
        let files = vec![
            ("A", "claims/bucket.yaml"),
            ("A", "modules/vpc.yaml"),
            ("A", "stacks/infrastructure.yaml"),
            ("A", "docs/README.md"),
        ];

        // Simulate FILE_PATH_PREFIX="claims/,modules/"
        let prefixes = vec!["claims/", "modules/"];

        let filtered: Vec<_> = files
            .into_iter()
            .filter(|(_, path)| prefixes.iter().any(|prefix| path.starts_with(prefix)))
            .collect();

        assert_eq!(filtered.len(), 2);
        assert!(filtered
            .iter()
            .any(|(_, p)| p.contains("claims/bucket.yaml")));
        assert!(filtered.iter().any(|(_, p)| p.contains("modules/vpc.yaml")));
        assert!(!filtered.iter().any(|(_, p)| p.contains("stacks/")));
        assert!(!filtered.iter().any(|(_, p)| p.contains("docs/")));
    }

    #[test]
    fn test_file_status_classification() {
        // Test that we correctly classify file statuses
        let statuses = vec![
            ("A", true),  // Added - should be in active_files
            ("M", true),  // Modified - should be in both active and deleted
            ("D", false), // Deleted - should only be in deleted_files
            ("R", false), // Renamed - should be ignored
        ];

        for (status, should_be_active) in statuses {
            let is_active = matches!(status, "A" | "M");
            assert_eq!(is_active, should_be_active);
        }
    }

    #[test]
    fn test_file_change_structure() {
        // Test that FileChange can be created correctly
        let file = FileChange {
            path: "claims/test.yaml".to_string(),
            content: "apiVersion: infraweave.io/v1\nkind: S3Bucket".to_string(),
        };

        assert_eq!(file.path, "claims/test.yaml");
        assert!(file.content.contains("apiVersion"));
    }

    #[test]
    fn test_processed_files_empty() {
        // Test empty ProcessedFiles
        let processed = ProcessedFiles {
            active_files: vec![],
            deleted_files: vec![],
        };

        assert_eq!(processed.active_files.len(), 0);
        assert_eq!(processed.deleted_files.len(), 0);
    }

    #[test]
    fn test_processed_files_with_data() {
        // Test ProcessedFiles with actual data
        let active = FileChange {
            path: "claims/bucket.yaml".to_string(),
            content: "apiVersion: infraweave.io/v1\nkind: S3Bucket".to_string(),
        };

        let deleted = FileChange {
            path: "claims/old-bucket.yaml".to_string(),
            content: "apiVersion: infraweave.io/v1\nkind: S3Bucket".to_string(),
        };

        let processed = ProcessedFiles {
            active_files: vec![active],
            deleted_files: vec![deleted],
        };

        assert_eq!(processed.active_files.len(), 1);
        assert_eq!(processed.deleted_files.len(), 1);
        assert_eq!(processed.active_files[0].path, "claims/bucket.yaml");
        assert_eq!(processed.deleted_files[0].path, "claims/old-bucket.yaml");
    }

    #[test]
    #[ignore] // Requires git repository with commits
    fn test_get_diff_integration() {
        // Integration test - requires actual git repo
        // Run with: cargo test -- --ignored

        // This assumes you're in a git repo with at least one commit
        let result = get_diff("HEAD~1", "HEAD");
        assert!(result.is_ok());
    }
}
