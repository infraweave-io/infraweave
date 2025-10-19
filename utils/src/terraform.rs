use env_defs::{ApiInfraPayload, ExtraData, TfLockProvider};
use std::fs::{write, File};

pub fn get_provider_url_key(
    tf_lock_provider: &TfLockProvider,
    target: &str,
    category: &str,
) -> (String, String) {
    let parts: Vec<&str> = tf_lock_provider.source.split('/').collect();
    // parts: ["registry.terraform.io", "hashicorp", "aws"]
    let namespace = parts[1];
    let provider = parts[2];

    let prefix = format!(
        "terraform-provider-{provider}_{version}",
        provider = provider,
        version = tf_lock_provider.version
    );
    let file = match category {
        // "index_json" => format!("index.json"),
        "provider_binary" => format!("{prefix}_{target}.zip"),
        "shasum" => format!("{prefix}_SHA256SUMS"),
        "signature" => format!("{prefix}_SHA256SUMS.72D7468F.sig"), // New Hashicorp signature after incident HCSEC-2021-12 (v0.15.1 and later)
        _ => panic!("Invalid category"),
    };

    let download_url = format!(
        "https://releases.hashicorp.com/terraform-provider-{provider}/{version}/{file}",
        version = tf_lock_provider.version,
    );
    let key = format!("registry.terraform.io/{namespace}/{provider}/{file}",);
    (download_url, key)
}

pub fn store_tf_vars_json(tf_vars: &serde_json::Value, folder_path: &str) {
    // Try to create a file
    let tf_vars_file = match File::create(format!("{}/terraform.tfvars.json", folder_path)) {
        Ok(file) => file,
        Err(e) => {
            eprintln!("Failed to create terraform.tfvars.json: {:?}", e);
            std::process::exit(1);
        }
    };

    // Write the JSON data to the file
    if let Err(e) = serde_json::to_writer_pretty(tf_vars_file, &tf_vars) {
        eprintln!("Failed to write JSON to terraform.tfvars.json: {:?}", e);
        std::process::exit(1);
    }
}

pub async fn store_backend_file(
    backend_provider: &str,
    folder_path: &str,
    extras_map: &serde_json::Value,
) {
    // There are verifications when publishing a module to ensure that there
    // is no existing already backend specified. This is to ensure that InfraWeave
    // uses its backend storage
    let backend_file_content = format!(
        r#"
terraform {{
    backend "{}" {{{}}}
}}"#,
        backend_provider,
        extras_map.as_object().map_or("".to_string(), |extras| {
            extras
                .iter()
                .map(|(k, v)| format!("\n        {} = {}", k, v))
                .collect::<Vec<String>>()
                .join("")
        }) + if !(extras_map == &serde_json::json!({})) {
            "\n    "
        } else {
            ""
        }
    );

    let path = format!("{}/backend.tf", folder_path);
    let file_path = std::path::Path::new(path.as_str());
    if let Err(e) = write(file_path, &backend_file_content) {
        eprintln!("Failed to write to backend.tf: {:?}", e);
        std::process::exit(1);
    }
}

#[rustfmt::skip]
pub fn get_extra_environment_variables(
    payload: &ApiInfraPayload,
) -> std::collections::HashMap<String, String> {
    get_extra_environment_variables_all(
        &payload.deployment_id,
        &payload.environment,
        &payload.reference,
        &payload.module_version,
        &payload.module_type,
        &payload.module_track,
        &payload.drift_detection,
        &payload.extra_data,
    )
}

#[rustfmt::skip]
pub fn     get_extra_environment_variables_all(
    deployment_id: &str,
    environment: &str,
    reference: &str,
    module_version: &str,
    module_type: &str,
    module_track: &str,
    drift_detection: &env_defs::DriftDetection,
    extra_data: &ExtraData,
) -> std::collections::HashMap<String, String> {
    let mut env_vars = std::collections::HashMap::new();
    env_vars.insert("INFRAWEAVE_DEPLOYMENT_ID".to_string(), deployment_id.to_string());
    env_vars.insert("INFRAWEAVE_ENVIRONMENT".to_string(), environment.to_string());
    env_vars.insert("INFRAWEAVE_REFERENCE".to_string(), reference.to_string());
    env_vars.insert("INFRAWEAVE_MODULE_VERSION".to_string(), module_version.to_string());
    env_vars.insert("INFRAWEAVE_MODULE_TYPE".to_string(), module_type.to_string());
    env_vars.insert("INFRAWEAVE_MODULE_TRACK".to_string(), module_track.to_string());
    env_vars.insert("INFRAWEAVE_DRIFT_DETECTION".to_string(), (if drift_detection.enabled {"enabled"} else {"disabled"}).to_string());
    env_vars.insert("INFRAWEAVE_DRIFT_DETECTION_INTERVAL".to_string(), if drift_detection.enabled {drift_detection.interval.to_string()} else {"N/A".to_string()});

    match &extra_data {
        ExtraData::GitHub(github_data) => {
            env_vars.insert("INFRAWEAVE_GIT_COMMITTER_EMAIL".to_string(), github_data.user.email.clone());
            env_vars.insert("INFRAWEAVE_GIT_COMMITTER_NAME".to_string(), github_data.user.name.clone());
            env_vars.insert("INFRAWEAVE_GIT_ACTOR_USERNAME".to_string(), github_data.user.username.clone());
            env_vars.insert("INFRAWEAVE_GIT_ACTOR_PROFILE_URL".to_string(), github_data.user.profile_url.clone());
            env_vars.insert("INFRAWEAVE_GIT_REPOSITORY_NAME".to_string(), github_data.repository.full_name.clone());
            env_vars.insert("INFRAWEAVE_GIT_REPOSITORY_PATH".to_string(), github_data.job_details.file_path.clone());
            env_vars.insert("INFRAWEAVE_GIT_COMMIT_SHA".to_string(), github_data.check_run.head_sha.clone());
        },  
        ExtraData::GitLab(gitlab_data) => {
            // TODO: Add more here for GitLab
            env_vars.insert("INFRAWEAVE_GIT_REPOSITORY_PATH".to_string(), gitlab_data.job_details.file_path.clone());
        },
        ExtraData::None => {}
    };
    env_vars
}
