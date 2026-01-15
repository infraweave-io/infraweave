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

    // In test mode, use MinIO for state storage instead of real S3
    #[cfg(feature = "test-mode")]
    {
        let minio_endpoint =
            env::var("MINIO_ENDPOINT").expect("MINIO_ENDPOINT must be set in test-mode");
        let minio_access_key =
            env::var("MINIO_ACCESS_KEY").expect("MINIO_ACCESS_KEY must be set in test-mode");
        let minio_secret_key =
            env::var("MINIO_SECRET_KEY").expect("MINIO_SECRET_KEY must be set in test-mode");

        exec.arg(format!("-backend-config=endpoint={}", minio_endpoint));
        exec.arg(format!("-backend-config=access_key={}", minio_access_key));
        exec.arg(format!("-backend-config=secret_key={}", minio_secret_key));
        exec.arg("-backend-config=skip_credentials_validation=true");
        exec.arg("-backend-config=skip_metadata_api_check=true");
        exec.arg("-backend-config=skip_requesting_account_id=true");
        exec.arg("-backend-config=use_path_style=true");
        // Skip DynamoDB locking in test mode - tests run sequentially so no conflicts
    }

    // Use DynamoDB locking only in production (not in test mode)
    #[cfg(not(feature = "test-mode"))]
    {
        let dynamodb_table = get_env_var("TF_DYNAMODB_TABLE");
        exec.arg(format!("-backend-config=dynamodb_table={}", dynamodb_table));
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
