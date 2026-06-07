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
    exec.arg(format!("-backend-config=bucket={}", tf_bucket));
    exec.arg(format!("-backend-config=key={}", key));
    exec.arg(format!("-backend-config=region={}", region));

    if let Some(s3_endpoint) = env_utils::runtime_env::s3_endpoint() {
        let access_key = env::var("AWS_ACCESS_KEY_ID")
            .or_else(|_| env::var("MINIO_ACCESS_KEY"))
            .unwrap_or_else(|_| "minio".to_string());
        let secret_key = env::var("AWS_SECRET_ACCESS_KEY")
            .or_else(|_| env::var("MINIO_SECRET_KEY"))
            .unwrap_or_else(|_| "minio123".to_string());

        exec.arg(format!("-backend-config=endpoint={}", s3_endpoint));
        exec.arg(format!("-backend-config=access_key={}", access_key));
        exec.arg(format!("-backend-config=secret_key={}", secret_key));

        if env_utils::runtime_env::is_local_s3_endpoint(&s3_endpoint) {
            exec.arg("-backend-config=skip_credentials_validation=true");
            exec.arg("-backend-config=skip_metadata_api_check=true");
            exec.arg("-backend-config=skip_requesting_account_id=true");
            exec.arg("-backend-config=use_path_style=true");
        }
    }
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
