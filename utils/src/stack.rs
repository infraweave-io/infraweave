use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;
use env_defs::DeploymentManifest;
use walkdir::WalkDir;

/// Reads all .yaml files in a given directory and returns the deployments.
pub fn read_stack_directory(directory: &Path) -> anyhow::Result<Vec<DeploymentManifest>> {
    let mut deployments = vec![];

    for entry in WalkDir::new(directory)
        .max_depth(10)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| {
            e.file_type().is_file() && e.path().extension().map_or(false, |ext| ext == "yaml")
        })
        .filter(|e| {
            e.path()
                .file_name()
                .and_then(|f| f.to_str())
                .map_or(false, |f| f != "stack.yaml")
        })
    {
        let content = fs::read_to_string(entry.path())?;

        // push if it's a deployment, otherwise continue
        let deployment: DeploymentManifest = match serde_yaml::from_str(&content) {
            Ok(deployment) => deployment,
            Err(e) => {
                println!("Failed to parse deployment {:?}: {}", entry.path(), e);
                continue;
            }
        };
        deployments.push(deployment);
    }

    anyhow::Ok(deployments)
}

pub fn to_snake_case(s: &str) -> String {
    let mut result = String::new();
    for (i, ch) in s.chars().enumerate() {
        if ch.is_uppercase() {
            if i != 0 {
                result.push('_');
            }
            result.push(ch.to_ascii_lowercase());
        } else {
            result.push(ch);
        }
    }
    result
}

pub fn from_snake_case(s: &str) -> String {
    let mut result = String::new();
    let mut chars = s.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '_' {
            if let Some(next) = chars.next() {
                result.push(next.to_ascii_uppercase());
            }
        } else {
            result.push(ch);
        }
    }
    result
}
