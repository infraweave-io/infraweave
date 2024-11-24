use env_defs::ModuleManifest;
use serde_yaml;
use tokio::fs::File;
use tokio::io::AsyncReadExt;

pub async fn read_module_from_file(
    file_path: &str,
) -> Result<ModuleManifest, Box<dyn std::error::Error>> {
    let mut file = File::open(file_path).await?;
    let mut contents = String::new();
    file.read_to_string(&mut contents).await?;
    let module: ModuleManifest = serde_yaml::from_str(&contents)?;
    Ok(module)
}
