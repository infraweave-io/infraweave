use env_defs::DeploymentManifest;
use std::collections::HashMap;

use crate::defs::{FileChange, GroupKey, GroupedFile, ManifestChange, ProcessedFiles};

fn extract_manifest_changes(file: &FileChange) -> Vec<ManifestChange> {
    let mut changes = Vec::new();
    for doc in file.content.split("---") {
        let doc = doc.trim();
        if doc.is_empty() {
            continue;
        }
        if let Ok(manifest) = serde_yaml::from_str::<DeploymentManifest>(doc) {
            let key = GroupKey {
                api_version: manifest.api_version.clone(),
                kind: manifest.kind.clone(),
                name: manifest.metadata.name.clone(),
                namespace: manifest
                    .metadata
                    .namespace
                    .clone()
                    .unwrap_or_else(|| "default".to_string()),
            };
            if let Ok(canonical) = serde_yaml::to_string(&manifest) {
                changes.push(ManifestChange {
                    key,
                    content: canonical,
                    file: file.clone(),
                });
            }
        } else {
            log::warn!(
                "Failed to parse manifest from file: {}\nManifest content: {}",
                file.path,
                file.content
            );
        }
    }
    changes
}

pub fn group_files_by_manifest(processed: ProcessedFiles) -> Vec<GroupedFile> {
    let mut active_changes: HashMap<GroupKey, ManifestChange> = HashMap::new();
    for file in processed.active_files.iter() {
        for change in extract_manifest_changes(file) {
            // If multiple documents produce the same key, let the last one win.
            active_changes.insert(change.key.clone(), change);
        }
    }

    let mut deleted_changes: HashMap<GroupKey, ManifestChange> = HashMap::new();
    for file in processed.deleted_files.iter() {
        for change in extract_manifest_changes(file) {
            deleted_changes.insert(change.key.clone(), change);
        }
    }

    let mut groups = Vec::new();

    // Process keys from active changes.
    for (key, active_change) in active_changes.iter() {
        if let Some(deleted_change) = deleted_changes.get(key) {
            if active_change.content == deleted_change.content {
                // Pure rename (no content change) => do nothing.
                continue;
            } else {
                // Parse both manifests from the YAML.
                let active_manifest =
                    serde_yaml::from_str::<DeploymentManifest>(&active_change.content)
                        .expect("Active manifest should parse");
                let deleted_manifest =
                    serde_yaml::from_str::<DeploymentManifest>(&deleted_change.content)
                        .expect("Deleted manifest should parse");

                if active_manifest.spec.region != deleted_manifest.spec.region {
                    // Region differs, so include both active and deleted.
                    groups.push(GroupedFile {
                        key: key.clone(),
                        active: Some((active_change.file.clone(), active_change.content.clone())),
                        deleted: Some((
                            deleted_change.file.clone(),
                            deleted_change.content.clone(),
                        )),
                    });
                } else {
                    // Same region – only active (apply) is reported.
                    groups.push(GroupedFile {
                        key: key.clone(),
                        active: Some((active_change.file.clone(), active_change.content.clone())),
                        deleted: None,
                    });
                }
            }
        } else {
            groups.push(GroupedFile {
                key: key.clone(),
                active: Some((active_change.file.clone(), active_change.content.clone())),
                deleted: None,
            });
        }
    }

    // Process keys only in deleted changes.
    for (key, deleted_change) in deleted_changes.into_iter() {
        if !active_changes.contains_key(&key) {
            groups.push(GroupedFile {
                key,
                active: None,
                deleted: Some((deleted_change.file.clone(), deleted_change.content.clone())),
            });
        }
    }

    groups
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use std::collections::HashMap;

    // Helper: standard YAML content for a minimal InfraWeave claim.
    fn valid_manifest(
        api_version: &str,
        kind: &str,
        name: &str,
        namespace: Option<&str>,
        module_version: &str,
        region: &str,
    ) -> String {
        let ns = namespace.unwrap_or("default");
        format!(
            r#"apiVersion: {}
kind: {}
metadata:
  name: {}
  namespace: {}
spec:
  moduleVersion: {}
  region: {}
  variables: {{}}
"#,
            api_version, kind, name, ns, module_version, region
        )
    }

    #[test]
    fn test_active_file_only() {
        // Active file only (added/modified)
        let active = FileChange {
            path: "active.yaml".to_string(),
            content: valid_manifest(
                "infraweave.io/v1",
                "Minimal",
                "my-minimal",
                Some("infraweave_cli"),
                "0.0.4-dev",
                "us-west-2",
            ),
        };
        let processed = ProcessedFiles {
            active_files: vec![active.clone()],
            deleted_files: vec![],
        };
        let groups = group_files_by_manifest(processed);
        assert_eq!(groups.len(), 1);

        let group = &groups[0];
        assert!(group.active.is_some());
        assert!(group.deleted.is_none());

        let key = &group.key;
        assert_eq!(key.api_version, "infraweave.io/v1");
        assert_eq!(key.kind, "Minimal");
        assert_eq!(key.name, "my-minimal");
        assert_eq!(key.namespace, "infraweave_cli");
    }

    #[test]
    fn test_deleted_file_only() {
        // Deleted file only
        let deleted = FileChange {
            path: "deleted.yaml".to_string(),
            content: valid_manifest(
                "infraweave.io/v1",
                "Minimal",
                "deleted-file",
                Some("default"),
                "1.0.0",
                "us-west-2",
            ),
        };
        let processed = ProcessedFiles {
            active_files: vec![],
            deleted_files: vec![deleted.clone()],
        };
        let groups = group_files_by_manifest(processed);
        assert_eq!(groups.len(), 1);

        let group = &groups[0];
        assert!(group.active.is_none());
        assert!(group.deleted.is_some());

        let key = &group.key;
        assert_eq!(key.api_version, "infraweave.io/v1");
        assert_eq!(key.kind, "Minimal");
        assert_eq!(key.name, "deleted-file");
        assert_eq!(key.namespace, "default");
    }

    #[test]
    fn test_renamed_file_only() {
        // Simulate a renamed file: one file appears as deleted (old name) and one as active (new name),
        // with the same manifest identity.
        let active = FileChange {
            path: "new.yaml".to_string(),
            content: valid_manifest(
                "infraweave.io/v1",
                "Minimal",
                "minimal1",
                Some("default"),
                "1.0.0",
                "us-west-2",
            ),
        };
        let deleted = FileChange {
            path: "old.yaml".to_string(),
            // If the content doesnt differs at all it should not be grouped.
            content: valid_manifest(
                "infraweave.io/v1",
                "Minimal",
                "minimal1",
                Some("default"),
                "1.0.0",
                "us-west-2",
            ),
        };
        let processed = ProcessedFiles {
            active_files: vec![active.clone()],
            deleted_files: vec![deleted.clone()],
        };
        let groups = group_files_by_manifest(processed);
        // In a pure rename (active and deleted for the same manifest), we expect no group.
        assert_eq!(groups.len(), 0);
    }

    #[test]
    fn test_renamed_file_and_modified_with_upgrade() {
        // Simulate a rename where the old file (deleted) has a different manifest version than the new (active) file. (which is the same manifest)
        let active = FileChange {
            path: "new.yaml".to_string(),
            // New active file has version "2.0.0"
            content: valid_manifest(
                "infraweave.io/v1",
                "Minimal",
                "minimal1",
                Some("default"),
                "2.0.0",
                "us-west-2",
            ),
        };
        let deleted = FileChange {
            path: "old.yaml".to_string(),
            // Deleted file has version "1.0.0"
            content: valid_manifest(
                "infraweave.io/v1",
                "Minimal",
                "minimal1",
                Some("default"),
                "1.0.0",
                "us-west-2",
            ),
        };
        let processed = ProcessedFiles {
            active_files: vec![active.clone()],
            deleted_files: vec![deleted.clone()],
        };
        let groups = group_files_by_manifest(processed);

        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].active.is_some(), true);
        assert_eq!(groups[0].deleted.is_none(), true);
    }

    #[test]
    fn test_renamed_file_and_modified_as_new() {
        // Simulate a rename where the old file (deleted) has a different manifest name than the new (active) file. (which is a new manifest)
        let active = FileChange {
            path: "new.yaml".to_string(),
            // New active file has name "minimal2"
            content: valid_manifest(
                "infraweave.io/v1",
                "Minimal",
                "minimal2",
                Some("default"),
                "1.0.0",
                "us-west-2",
            ),
        };
        let deleted = FileChange {
            path: "old.yaml".to_string(),
            // Deleted file has name "minimal1"
            content: valid_manifest(
                "infraweave.io/v1",
                "Minimal",
                "minimal1",
                Some("default"),
                "1.0.0",
                "us-west-2",
            ),
        };
        let processed = ProcessedFiles {
            active_files: vec![active.clone()],
            deleted_files: vec![deleted.clone()],
        };
        let groups = group_files_by_manifest(processed);
        // We expect two separate groups now.
        assert_eq!(groups.len(), 2);

        let mut found_active = false;
        let mut found_deleted = false;
        for group in groups {
            if let Some((active_file, _yaml)) = group.active {
                if group.key.name == "minimal2" && active_file.path == "new.yaml" {
                    found_active = true;
                }
            }
            if let Some((deleted_file, _yaml)) = group.deleted {
                if group.key.name == "minimal1" && deleted_file.path == "old.yaml" {
                    found_deleted = true;
                }
            }
        }
        assert!(found_active, "Expected active group for minimal2 not found");
        assert!(
            found_deleted,
            "Expected deleted group for minimal1 not found"
        );
    }

    #[test]
    fn test_multiple_files() {
        let active1 = FileChange {
            path: "file1.yaml".to_string(),
            content: valid_manifest(
                "infraweave.io/v1",
                "Minimal",
                "file1",
                Some("ns1"),
                "1.0.0",
                "us-west-2",
            ),
        };
        let active2 = FileChange {
            path: "file2.yaml".to_string(),
            // No namespace provided => defaults to "default"
            content: valid_manifest(
                "infraweave.io/v1",
                "Minimal",
                "file2",
                None,
                "1.1.0",
                "us-west-2",
            ),
        };
        let deleted = FileChange {
            path: "file3.yaml".to_string(),
            content: valid_manifest(
                "infraweave.io/v1",
                "Minimal",
                "file3",
                Some("ns3"),
                "2.0.0",
                "us-west-2",
            ),
        };

        let processed = ProcessedFiles {
            active_files: vec![active1.clone(), active2.clone()],
            deleted_files: vec![deleted.clone()],
        };

        let groups = group_files_by_manifest(processed);
        // We expect three separate groups.
        assert_eq!(groups.len(), 3);
        // Verify each group's manifest identity.
        let mut group_map: HashMap<String, GroupKey> = HashMap::new();
        for group in groups {
            group_map.insert(group.key.name.clone(), group.key);
        }
        let key1 = group_map.get("file1").expect("Missing group for file1");
        assert_eq!(key1.namespace, "ns1");

        let key2 = group_map.get("file2").expect("Missing group for file2");
        assert_eq!(key2.namespace, "default");

        let key3 = group_map.get("file3").expect("Missing group for file3");
        assert_eq!(key3.namespace, "ns3");
    }

    #[test]
    fn test_invalid_yaml() {
        // File content that is not valid YAML.
        let active_invalid = FileChange {
            path: "invalid.yaml".to_string(),
            content: "this is not valid yaml".to_string(),
        };

        let processed = ProcessedFiles {
            active_files: vec![active_invalid],
            deleted_files: vec![],
        };

        let groups = group_files_by_manifest(processed);
        // Expect no group to be created since YAML parsing fails.
        assert_eq!(groups.len(), 0);
    }

    #[test]
    fn test_invalid_and_valid_yaml() {
        // File content that is not valid YAML.
        let active_invalid = FileChange {
            path: "invalid.yaml".to_string(),
            content: "this is not valid yaml".to_string(),
        };
        let active_valid = FileChange {
            path: "file1.yaml".to_string(),
            content: valid_manifest(
                "infraweave.io/v1",
                "Minimal",
                "file1",
                Some("ns1"),
                "1.0.0",
                "us-west-2",
            ),
        };

        let processed = ProcessedFiles {
            active_files: vec![active_invalid, active_valid],
            deleted_files: vec![],
        };

        let groups = group_files_by_manifest(processed);
        // Expect one group to be created since YAML parsing fails for the first document and succeeds for the second.
        assert_eq!(groups.len(), 1);
    }

    // Multidoc tests

    #[test]
    fn test_multidoc_active_only() {
        // Create a multi-document active file with two different manifests.
        let doc1 = valid_manifest(
            "infraweave.io/v1",
            "Minimal",
            "doc1",
            Some("default"),
            "1.0.0",
            "us-west-2",
        );
        let doc2 = valid_manifest(
            "infraweave.io/v1",
            "Minimal",
            "doc2",
            Some("default"),
            "1.0.0",
            "us-west-2",
        );
        // Concatenate the two documents separated by '---'
        let multi_yaml = format!("{}\n---\n{}", doc1, doc2);

        let active = FileChange {
            path: "multidoc.yaml".to_string(),
            content: multi_yaml,
        };
        let processed = ProcessedFiles {
            active_files: vec![active],
            deleted_files: vec![],
        };
        let groups = group_files_by_manifest(processed);
        // Expect 2 groups: one for "doc1" and one for "doc2"
        assert_eq!(groups.len(), 2);
        let names: Vec<_> = groups.iter().map(|g| g.key.name.clone()).collect();
        assert!(names.contains(&"doc1".to_string()));
        assert!(names.contains(&"doc2".to_string()));
    }

    #[test]
    fn test_multidoc_pure_rename() {
        // Active and deleted files are multi-document and identical.
        let doc1 = valid_manifest(
            "infraweave.io/v1",
            "Minimal",
            "doc1",
            Some("default"),
            "1.0.0",
            "us-west-2",
        );
        let doc2 = valid_manifest(
            "infraweave.io/v1",
            "Minimal",
            "doc2",
            Some("default"),
            "1.0.0",
            "us-west-2",
        );
        let multi_yaml = format!("{}\n---\n{}", doc1, doc2);

        let active = FileChange {
            path: "active_multidoc.yaml".to_string(),
            content: multi_yaml.clone(),
        };
        let deleted = FileChange {
            path: "deleted_multidoc.yaml".to_string(),
            content: multi_yaml,
        };
        let processed = ProcessedFiles {
            active_files: vec![active],
            deleted_files: vec![deleted],
        };
        let groups = group_files_by_manifest(processed);
        // Both active and deleted documents are identical => pure rename => no group.
        assert_eq!(groups.len(), 0);
    }

    #[test]
    fn test_multidoc_upgrade() {
        // Simulate an upgrade for one document and no change for the other.
        let doc1_deleted = valid_manifest(
            "infraweave.io/v1",
            "Minimal",
            "doc1",
            Some("default"),
            "1.0.0",
            "us-west-2",
        );
        let doc2 = valid_manifest(
            "infraweave.io/v1",
            "Minimal",
            "doc2",
            Some("default"),
            "1.0.0",
            "us-west-2",
        );
        let deleted_yaml = format!("{}\n---\n{}", doc1_deleted, doc2);

        let doc1_active = valid_manifest(
            "infraweave.io/v1",
            "Minimal",
            "doc1",
            Some("default"),
            "2.0.0",
            "us-west-2",
        ); // upgraded version
        let active_yaml = format!("{}\n---\n{}", doc1_active, doc2.clone());

        let active = FileChange {
            path: "active_multidoc.yaml".to_string(),
            content: active_yaml,
        };
        let deleted = FileChange {
            path: "deleted_multidoc.yaml".to_string(),
            content: deleted_yaml,
        };
        let processed = ProcessedFiles {
            active_files: vec![active],
            deleted_files: vec![deleted],
        };
        let groups = group_files_by_manifest(processed);
        // For "doc1": content differs => upgrade => expect one group (active only)
        // For "doc2": pure rename => expect no group.
        assert_eq!(groups.len(), 1);
        let group = &groups[0];
        assert_eq!(group.key.name, "doc1");
        assert!(group.active.is_some());
        assert!(group.deleted.is_none());
    }

    #[test]
    fn test_multidoc_mixed() {
        // Active file contains two documents; deleted file contains only the first document.
        let doc1 = valid_manifest(
            "infraweave.io/v1",
            "Minimal",
            "doc1",
            Some("default"),
            "1.0.0",
            "us-west-2",
        );
        let doc2 = valid_manifest(
            "infraweave.io/v1",
            "Minimal",
            "doc2",
            Some("default"),
            "1.0.0",
            "us-west-2",
        );
        let active_yaml = format!("{}\n---\n{}", doc1.clone(), doc2.clone());
        let deleted_yaml = doc1.clone(); // only doc1 present in deleted
        let active = FileChange {
            path: "active_multidoc.yaml".to_string(),
            content: active_yaml,
        };
        let deleted = FileChange {
            path: "deleted_multidoc.yaml".to_string(),
            content: deleted_yaml,
        };
        let processed = ProcessedFiles {
            active_files: vec![active],
            deleted_files: vec![deleted],
        };
        let groups = group_files_by_manifest(processed);
        // For doc1: pure rename (active and deleted identical) => drop it.
        // For doc2: active only => one group.
        assert_eq!(groups.len(), 1);
        let group = &groups[0];
        assert_eq!(group.key.name, "doc2");
        assert!(group.active.is_some());
        assert!(group.deleted.is_none());
    }

    #[test]
    fn test_invalid_and_valid_multidoc() {
        // One file with invalid YAML and one with valid multi-doc YAML.
        let invalid = "this is not valid yaml".to_string();
        let active_invalid = FileChange {
            path: "invalid.yaml".to_string(),
            content: invalid,
        };
        let doc = valid_manifest(
            "infraweave.io/v1",
            "Minimal",
            "file1",
            Some("ns1"),
            "1.0.0",
            "us-west-2",
        );
        let active_valid = FileChange {
            path: "file1.yaml".to_string(),
            content: doc,
        };

        let processed = ProcessedFiles {
            active_files: vec![active_invalid, active_valid],
            deleted_files: vec![],
        };

        let groups = group_files_by_manifest(processed);
        // The invalid file should be skipped; only the valid manifest should be grouped.
        assert_eq!(groups.len(), 1);
        let group = &groups[0];
        assert_eq!(group.key.name, "file1");
        assert_eq!(group.key.namespace, "ns1");
    }

    #[test]
    fn test_manifest_extension_new_and_deleted() {
        // Before state: two documents: one for "minimal" and one for "minimal2"
        let doc_minimal = valid_manifest(
            "infraweave.io/v1",
            "Minimal",
            "minimal",
            Some("ns4"),
            "0.0.1-dev",
            "us-west-2",
        );
        let doc_minimal2 = valid_manifest(
            "infraweave.io/v1",
            "Minimal",
            "minimal2",
            Some("ns4"),
            "0.0.1-dev",
            "us-west-2",
        );
        let before_multi_yaml = format!("{}\n---\n{}", doc_minimal, doc_minimal2);

        // After state: two documents: one unchanged for "minimal" and one for "minimal3"
        let doc_minimal_after = valid_manifest(
            "infraweave.io/v1",
            "Minimal",
            "minimal",
            Some("ns4"),
            "0.0.1-dev",
            "us-west-2",
        );
        let doc_minimal3 = valid_manifest(
            "infraweave.io/v1",
            "Minimal",
            "minimal3",
            Some("ns4"),
            "0.0.1-dev",
            "us-west-2",
        );
        let after_multi_yaml = format!("{}\n---\n{}", doc_minimal_after, doc_minimal3);

        // Create FileChange instances for the same file path.
        let active_file = FileChange {
            path: "another-claim4.yaml".to_string(),
            content: after_multi_yaml,
        };
        let deleted_file = FileChange {
            path: "another-claim4.yaml".to_string(),
            content: before_multi_yaml,
        };

        let processed = ProcessedFiles {
            active_files: vec![active_file],
            deleted_files: vec![deleted_file],
        };

        let groups = group_files_by_manifest(processed);
        // We expect two groups:
        //  - One group for the manifest "minimal2" found only in the deleted (before) state => should trigger a deletion.
        //  - One group for the manifest "minimal3" found only in the active (after) state => should trigger an apply.
        // The unchanged "minimal" cancels out and produces no group.
        assert_eq!(groups.len(), 2, "Expected exactly 2 groups");

        let mut active_found = false;
        let mut deleted_found = false;
        for group in groups {
            if let Some((active, _)) = group.active {
                if group.key.name == "minimal3" && active.path == "another-claim4.yaml" {
                    active_found = true;
                }
            }
            if let Some((deleted, _)) = group.deleted {
                if group.key.name == "minimal2" && deleted.path == "another-claim4.yaml" {
                    deleted_found = true;
                }
            }
        }
        assert!(active_found, "Expected active group for minimal3 not found");
        assert!(
            deleted_found,
            "Expected deleted group for minimal2 not found"
        );
    }

    #[test]
    fn test_only_region_modification() {
        let deleted = FileChange {
            path: "region_change.yaml".to_string(),
            content: valid_manifest(
                "infraweave.io/v1",
                "Minimal",
                "region-mod",
                Some("default"),
                "1.0.0",
                "us-west-2",
            ),
        };

        let active = FileChange {
            path: "region_change.yaml".to_string(),
            content: valid_manifest(
                "infraweave.io/v1",
                "Minimal",
                "region-mod",
                Some("default"),
                "1.0.0",
                "eu-central-1",
            ),
        };

        let processed = ProcessedFiles {
            active_files: vec![active.clone()],
            deleted_files: vec![deleted.clone()],
        };

        let groups = group_files_by_manifest(processed);

        // Expected behavior: since the manifest content differs due to the region change,
        // we want to see both the active file (with eu-central-1) and the deleted file (with us-west-2)
        // present in the resulting group.
        assert_eq!(
            groups.len(),
            1,
            "Expected one group for region modification"
        );

        let group = &groups[0];
        // The desired expectation is that both active and deleted changes are represented.
        assert!(
            group.active.is_some(),
            "Expected active file in the group for the updated region"
        );
        assert!(
            group.deleted.is_some(),
            "Expected deleted file in the group for the old region"
        );

        let (_, active_content) = group.active.as_ref().unwrap();
        let (_, deleted_content) = group.deleted.as_ref().unwrap();

        assert!(
            active_content.contains("eu-central-1"),
            "Active file should have region eu-central-1"
        );
        assert!(
            deleted_content.contains("us-west-2"),
            "Deleted file should have region us-west-2"
        );
    }

    #[test]
    fn test_content_and_region_modification() {
        let deleted = FileChange {
            path: "region_change.yaml".to_string(),
            content: valid_manifest(
                "infraweave.io/v1",
                "Minimal",
                "region-mod",
                Some("default"),
                "1.0.0",
                "us-west-2",
            ),
        };

        let active = FileChange {
            path: "region_change.yaml".to_string(),
            content: valid_manifest(
                "infraweave.io/v1",
                "Minimal",
                "region-mod",
                Some("default"),
                "1.0.1",
                "eu-central-1",
            ),
        };

        let processed = ProcessedFiles {
            active_files: vec![active.clone()],
            deleted_files: vec![deleted.clone()],
        };

        let groups = group_files_by_manifest(processed);

        // Expected behavior: since the manifest content differs due to the region change,
        // we want to see both the active file (with eu-central-1) and the deleted file (with us-west-2)
        // present in the resulting group.
        assert_eq!(
            groups.len(),
            1,
            "Expected one group for region modification"
        );

        let group = &groups[0];
        // The desired expectation is that both active and deleted changes are represented.
        assert!(
            group.active.is_some(),
            "Expected active file in the group for the updated region"
        );
        assert!(
            group.deleted.is_some(),
            "Expected deleted file in the group for the old region"
        );

        let (_, active_content) = group.active.as_ref().unwrap();
        let (_, deleted_content) = group.deleted.as_ref().unwrap();

        assert!(
            active_content.contains("eu-central-1"),
            "Active file should have region eu-central-1"
        );
        assert!(
            deleted_content.contains("us-west-2"),
            "Deleted file should have region us-west-2"
        );
    }
}
