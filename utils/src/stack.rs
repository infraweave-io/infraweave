use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;
use env_defs::DeploymentManifest;
use walkdir::WalkDir;
use inflector::Inflector;

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
    s.to_snake_case()
}

pub fn to_camel_case(s: &str) -> String {
    s.to_camel_case()
}
