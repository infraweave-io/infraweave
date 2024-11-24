#![allow(dead_code)]
pub async fn bootstrap_environment(_local: bool) -> Result<(), anyhow::Error> {
    Ok(())
}

pub async fn bootstrap_teardown_environment(
    _local: bool,
) -> Result<(), anyhow::Error> {
    Ok(())
}
