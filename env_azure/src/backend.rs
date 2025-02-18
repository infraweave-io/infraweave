use std::env;

pub async fn set_backend(
    exec: &mut tokio::process::Command,
    deployment_id: &str,
    environment: &str,
) {
    let account_id = get_env_var("ACCOUNT_ID");
    let tf_bucket = get_env_var("TF_BUCKET");
    let key = format!(
        "{}/{}/{}/terraform.tfstate",
        account_id, environment, deployment_id
    );
    let storage_account = get_env_var("STORAGE_ACCOUNT");
    let resource_group_name = get_env_var("RESOURCE_GROUP_NAME");

    // Storage account supports both locking and state storage
    exec.arg(format!(
        "-backend-config=storage_account_name={}",
        storage_account
    ));
    exec.arg(format!(
        "-backend-config=resource_group_name={}",
        resource_group_name
    ));
    exec.arg(format!("-backend-config=container_name={}", tf_bucket));
    exec.arg(format!("-backend-config=key={}", key));
    exec.arg(format!("-backend-config=subscription_id={}", account_id));
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
