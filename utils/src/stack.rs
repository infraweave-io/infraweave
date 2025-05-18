use env_defs::DeploymentManifest;
use std::fs;
use std::path::Path;
use walkdir::WalkDir;

/// Reads all .yaml files in a given directory and returns the deployments.
pub fn read_stack_directory(directory: &Path) -> anyhow::Result<Vec<DeploymentManifest>> {
    let mut deployments = vec![];

    for entry in WalkDir::new(directory)
        .max_depth(10)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| {
            e.file_type().is_file() && e.path().extension().is_some_and(|ext| ext == "yaml")
        })
        .filter(|e| {
            e.path()
                .file_name()
                .and_then(|f| f.to_str())
                .is_some_and(|f| f != "stack.yaml")
        })
    {
        let content = fs::read_to_string(entry.path())?;

        // add if it's a deployment, otherwise return early with an error
        let deployment: DeploymentManifest = match serde_yaml::from_str(&content) {
            Ok(deployment) => deployment,
            Err(e) => {
                println!("Failed to parse deployment {:?}: {}", entry.path(), e);
                anyhow::bail!("Failed to parse deployment {:?}: {}", entry.path(), e);
            }
        };
        deployments.push(deployment);
    }

    anyhow::Ok(deployments)
}
