use std::env;

pub async fn set_backend(
    exec: &mut tokio::process::Command,
    storage_basepath: &str,
    deployment_id: &str,
    environment: &str,
) {
    let tf_bucket = get_env_var("TF_BUCKET");
    let region = get_env_var("REGION");
    let key = format!(
        "{}{}/{}/terraform.tfstate",
        storage_basepath, environment, deployment_id
    );
    let dynamodb_table = get_env_var("TF_DYNAMODB_TABLE");
    exec.arg(format!("-backend-config=bucket={}", tf_bucket));
    exec.arg(format!("-backend-config=key={}", key));
    exec.arg(format!("-backend-config=region={}", region));
    exec.arg(format!("-backend-config=dynamodb_table={}", dynamodb_table));
}

fn get_env_var(key: &str) -> String {
    match env::var(key) {
        Ok(val) => val,
        Err(_) => {
            eprintln!("Environment variable {} is not set", key);
            std::process::exit(1);
        }
    }
}
