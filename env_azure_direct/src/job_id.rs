pub async fn get_current_job_id() -> Result<String, anyhow::Error> {
    let container_group_name =
        std::env::var("CONTAINER_GROUP_NAME").expect("CONTAINER_GROUP_NAME not set");

    eprintln!("Instance Name: {}", container_group_name);

    Ok(container_group_name)
}
