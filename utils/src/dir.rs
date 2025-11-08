pub fn create_temp_dir() -> std::io::Result<std::path::PathBuf> {
    let temp_dir = std::env::temp_dir();
    let dir_name = format!("temp_dir_{}", uuid::Uuid::new_v4());
    let temp_path = temp_dir.join(dir_name);

    std::fs::create_dir_all(&temp_path)?;

    Ok(temp_path)
}
