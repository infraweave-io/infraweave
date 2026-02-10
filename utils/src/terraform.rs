use anyhow::{anyhow, Context, Result};
use bollard::exec::StartExecResults;
use bollard::image::CreateImageOptions;
use env_defs::{ApiInfraPayload, ExtraData, TfLockProvider};
use log::warn;
use serde::Deserialize;
use serde_json::Value;
use std::fs::{write, File};
use uuid::Uuid;

#[derive(Deserialize, Debug)]
#[allow(dead_code)]
struct RegistryDownloadResponse {
    download_url: String,
    shasum: String,
    shasums_url: String,
    shasums_signature_url: String,
    filename: String,
}

/// Registry API hostname. Can be overridden with REGISTRY_API_HOSTNAME env var.
/// Defaults to registry.opentofu.org
/// Examples:
///   - registry.opentofu.org (default)
///   - registry.opentofu.org
///   - custom-registry.company.com
pub async fn get_provider_url_key(
    tf_lock_provider: &TfLockProvider,
    target: &str,
    category: &str,
) -> Result<(String, String)> {
    let parts: Vec<&str> = tf_lock_provider.source.split('/').collect();
    // parts: ["registry.opentofu.org", "hashicorp", "aws"]
    let namespace = parts[1];
    let provider = parts[2];

    // Parse target to extract os and arch (e.g., "darwin_arm64" -> "darwin", "arm64")
    let target_parts: Vec<&str> = target.split('_').collect();
    if target_parts.len() != 2 {
        anyhow::bail!("Invalid target format: {}", target);
    }
    let os = target_parts[0];
    let arch = target_parts[1];

    let registry_api_hostname = std::env::var("REGISTRY_API_HOSTNAME")
        .unwrap_or_else(|_| "registry.opentofu.org".to_string());

    // Query the Registry API
    let registry_url = format!(
        "https://{}/v1/providers/{}/{}/{}/download/{}/{}",
        registry_api_hostname, namespace, provider, tf_lock_provider.version, os, arch
    );

    let client = reqwest::Client::new();
    let response = client
        .get(&registry_url)
        .header("User-Agent", "infraweave")
        .send()
        .await
        .context("Failed to query Registry API")?;

    if !response.status().is_success() {
        anyhow::bail!("Registry API returned error: {}", response.status());
    }

    let registry_data: RegistryDownloadResponse = response
        .json()
        .await
        .context("Failed to parse Registry API response")?;

    let (download_url, file) = match category {
        "provider_binary" => (registry_data.download_url, registry_data.filename),
        "shasum" => {
            let filename = registry_data
                .shasums_url
                .split('/')
                .last()
                .unwrap_or("SHA256SUMS")
                .to_string();
            (registry_data.shasums_url, filename)
        }
        "signature" => {
            let filename = registry_data
                .shasums_signature_url
                .split('/')
                .last()
                .unwrap_or("SHA256SUMS.sig")
                .to_string();
            (registry_data.shasums_signature_url, filename)
        }
        _ => anyhow::bail!("Invalid category: {}", category),
    };

    let key = format!(
        "{}/{}/{}/{}",
        registry_api_hostname, namespace, provider, file
    );
    Ok((download_url, key))
}

use bollard::container::Config;
use bollard::container::CreateContainerOptions;
use bollard::container::StartContainerOptions;
use bollard::container::StopContainerOptions;
use bollard::Docker;
use futures_util::stream::StreamExt;

pub async fn run_terraform_provider_lock(temp_module_path: &Path) -> Result<String, anyhow::Error> {
    let docker = Docker::connect_with_local_defaults()?;

    let (id, name) = start_tf_container().await?;

    copy_module_to_container(&docker, &id, temp_module_path).await?;

    match exec_terraform(&docker, &id, &["init", "-no-color"]).await {
        Ok(init_output) => println!("Init command output:\n{}", init_output),
        Err(e) => {
            stop(&docker, &name).await?;
            return Err(e);
        }
    };

    match exec_terraform(&docker, &id, &["validate"]).await {
        Ok(validate_out) => println!("Validate command output:\n{}", validate_out),
        Err(e) => {
            stop(&docker, &name).await?;
            return Err(e);
        }
    };

    match exec(&docker, &id, "cat", &["/workspace/.terraform.lock.hcl"]).await {
        Ok(lockfile_content) => {
            let stop_request = stop(&docker, &name);
            println!("lockfile_content:\n{}", &lockfile_content);
            if let Err(e) = stop_request.await {
                warn!("Failed to stop and remove docker: {}", e);
            }
            Ok(lockfile_content)
        }
        Err(e) => {
            stop(&docker, &name).await?;
            return Err(e);
        }
    }
}

async fn stop(docker: &Docker, name: &String) -> Result<(), anyhow::Error> {
    let _ = docker
        .stop_container(&name, Some(StopContainerOptions { t: 0 }))
        .await?;
    Ok(())
}

use bollard::service::HostConfig;

async fn start_tf_container() -> anyhow::Result<(String, String)> {
    let docker = Docker::connect_with_local_defaults()?;

    // Allow overriding the Terraform image via environment variable
    let image = std::env::var("INFRAWEAVE_TF_IMAGE")
        .unwrap_or_else(|_| "ghcr.io/opentofu/opentofu:1".to_string());

    // 1) Ensure the image is present (pull if needed)
    let pull_opts = CreateImageOptions {
        from_image: image.as_str(),
        ..Default::default()
    };

    let mut pull_stream = docker.create_image(Some(pull_opts), None, None);
    while let Some(pull_result) = pull_stream.next().await {
        match pull_result {
            Ok(_) => {}
            Err(e) => return Err(anyhow::anyhow!("Failed to pull image: {}", e)),
        }
    }

    let name = format!("tf-run-{}", Uuid::new_v4());

    let config = Config {
        image: Some(image.as_str()),
        host_config: Some(HostConfig {
            auto_remove: Some(true),
            ..Default::default()
        }),
        entrypoint: Some(vec!["/bin/sh"]),
        working_dir: Some("/workspace"),
        cmd: Some(vec!["-c", "tail -f /dev/null"]),
        ..Default::default()
    };

    let container = docker
        .create_container(
            Some(CreateContainerOptions {
                name: &name,
                platform: None,
            }),
            config,
        )
        .await?;
    docker
        .start_container(&container.id, None::<StartContainerOptions<String>>)
        .await?;
    Ok((container.id, name))
}

use bollard::container::UploadToContainerOptions;
use std::path::Path;
use tar::Builder;

async fn copy_module_to_container(
    docker: &Docker,
    container_id: &str,
    module_path: &Path,
) -> anyhow::Result<()> {
    // 1) Build the tar in-memory
    let mut buf = Vec::new();
    {
        let mut tar = Builder::new(&mut buf);
        tar.append_dir_all(".", module_path)?;
        tar.finish()?;
    }

    // 2) Wrap the Vec<u8> as Bytes and call upload_to_container
    let tar_bytes = tokio_util::bytes::Bytes::from(buf);
    let opts = UploadToContainerOptions {
        path: "/workspace".to_string(),
        ..Default::default()
    };

    docker
        .upload_to_container(container_id, Some(opts), tar_bytes)
        .await?;

    Ok(())
}

use bollard::exec::CreateExecOptions;

async fn exec_terraform(
    docker: &Docker,
    container_id: &str,
    args: &[&str],
) -> anyhow::Result<String> {
    // Allow overriding the Terraform command via environment variable
    let cmd = std::env::var("INFRAWEAVE_TF_CMD").unwrap_or_else(|_| "tofu".to_string());
    exec(docker, container_id, &cmd, args).await
}

async fn exec(
    docker: &Docker,
    container_id: &str,
    cmd: &str,
    args: &[&str],
) -> anyhow::Result<String> {
    let exec = docker
        .create_exec(
            container_id,
            CreateExecOptions {
                cmd: Some(std::iter::once(cmd).chain(args.iter().copied()).collect()),
                attach_stdout: Some(true),
                attach_stderr: Some(true),
                working_dir: Some("/workspace".into()),
                ..Default::default()
            },
        )
        .await?;

    let mut output = match docker.start_exec(&exec.id, None).await? {
        StartExecResults::Attached { output, .. } => output,
        _ => {
            return Ok(String::new());
        }
    };

    let mut raw = Vec::new();
    while let Some(frame) = output.next().await {
        match frame {
            Ok(bollard::container::LogOutput::StdOut { message })
            | Ok(bollard::container::LogOutput::StdErr { message }) => {
                raw.extend_from_slice(&message);
            }
            Ok(_) => {}
            Err(e) => eprintln!("exec error: {}", e),
        }
    }

    let text = String::from_utf8_lossy(&raw).into_owned();

    let exec_inspect = docker.inspect_exec(&exec.id).await?;
    if let Some(error_code) = exec_inspect.exit_code {
        if error_code != 0 {
            return Err(anyhow!(format!(
                "{} {} failed with exit code {} validate message {}",
                cmd,
                args.join(" "),
                error_code,
                text
            )));
        }
    }

    Ok(text)
}

#[derive(Debug, Clone)]
pub struct DestructiveChange {
    pub address: String,
    pub action: String, // "delete" or "replace"
}

pub fn plan_get_destructive_changes(plan_json: &Value) -> Vec<DestructiveChange> {
    plan_json
        .get("resource_changes")
        .and_then(|v| v.as_array())
        .map(|changes| {
            changes
                .iter()
                .filter_map(extract_destructive_change)
                .collect()
        })
        .unwrap_or_default()
}

fn extract_destructive_change(resource_change: &Value) -> Option<DestructiveChange> {
    // Terraform JSON structure: resource_change.change.actions
    // where "change" is a field containing the actual change details
    let actions = resource_change.get("change")?.get("actions")?.as_array()?;

    // Only process if this change includes a delete action
    if !actions.iter().any(|a| a.as_str() == Some("delete")) {
        return None;
    }

    let address = resource_change
        .get("address")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown")
        .to_string();

    let action = if actions.len() > 1 {
        "replace"
    } else {
        "delete"
    }
    .to_string();

    Some(DestructiveChange { address, action })
}

#[cfg(test)]
mod provider_tests {
    use super::*;
    use env_defs::TfLockProvider;
    use pretty_assertions::assert_eq;

    #[tokio::test]
    async fn test_get_provider_url_key_aws_provider_binary() {
        let tf_lock_provider = TfLockProvider {
            source: "registry.opentofu.org/hashicorp/aws".to_string(),
            version: "5.0.0".to_string(),
        };
        let target = "linux_amd64";
        let category = "provider_binary";

        let result = get_provider_url_key(&tf_lock_provider, target, category).await;
        assert!(result.is_ok(), "Failed to get provider URL: {:?}", result);

        let (download_url, key) = result.unwrap();

        println!("AWS provider download URL: {}", download_url);
        println!("AWS provider key: {}", key);

        assert!(
            download_url.contains("github.com/opentofu/terraform-provider-aws/releases/download"),
            "Expected github release URL, got: {}",
            download_url
        );
        assert!(
            download_url.contains("terraform-provider-aws"),
            "Expected provider name in URL, got: {}",
            download_url
        );
        assert!(
            download_url.contains("5.0.0"),
            "Expected version in URL, got: {}",
            download_url
        );
        assert!(
            download_url.ends_with(".zip"),
            "Expected .zip extension, got: {}",
            download_url
        );

        assert!(
            key.starts_with("registry.opentofu.org/hashicorp/aws/"),
            "Expected key to start with registry path, got: {}",
            key
        );
        assert!(
            key.ends_with(".zip"),
            "Expected key to end with .zip, got: {}",
            key
        );
    }

    #[tokio::test]
    async fn test_get_provider_url_key_aws_shasum() {
        let tf_lock_provider = TfLockProvider {
            source: "registry.opentofu.org/hashicorp/aws".to_string(),
            version: "5.0.0".to_string(),
        };
        let target = "linux_amd64";
        let category = "shasum";

        let result = get_provider_url_key(&tf_lock_provider, target, category).await;
        assert!(result.is_ok(), "Failed to get shasum URL: {:?}", result);

        let (download_url, key) = result.unwrap();

        assert!(
            download_url.contains("SHA256SUMS"),
            "Expected SHA256SUMS in URL, got: {}",
            download_url
        );

        assert!(
            key.contains("SHA256SUMS"),
            "Expected SHA256SUMS in key, got: {}",
            key
        );
    }

    #[tokio::test]
    async fn test_get_provider_url_key_aws_signature() {
        let tf_lock_provider = TfLockProvider {
            source: "registry.opentofu.org/hashicorp/aws".to_string(),
            version: "5.0.0".to_string(),
        };
        let target = "linux_amd64";
        let category = "signature";

        let result = get_provider_url_key(&tf_lock_provider, target, category).await;
        assert!(result.is_ok(), "Failed to get signature URL: {:?}", result);

        let (download_url, key) = result.unwrap();

        assert!(
            download_url.contains("SHA256SUMS") && download_url.contains(".sig"),
            "Expected SHA256SUMS.sig in URL, got: {}",
            download_url
        );

        assert!(
            key.contains("SHA256SUMS") && key.contains(".sig"),
            "Expected SHA256SUMS.sig in key, got: {}",
            key
        );
    }

    #[tokio::test]
    async fn test_get_provider_url_key_docker_provider() {
        let tf_lock_provider = TfLockProvider {
            source: "registry.opentofu.org/kreuzwerker/docker".to_string(),
            version: "3.0.2".to_string(),
        };
        let target = "linux_amd64";
        let category = "provider_binary";

        let result = get_provider_url_key(&tf_lock_provider, target, category).await;
        assert!(
            result.is_ok(),
            "Failed to get Docker provider URL: {:?}",
            result
        );

        let (download_url, key) = result.unwrap();

        println!("Docker provider download URL: {}", download_url);
        println!("Docker provider key: {}", key);

        assert!(
            download_url.contains("github.com") || download_url.contains("releases"),
            "Expected GitHub releases URL for Docker provider, got: {}",
            download_url
        );
        assert!(
            download_url.contains("terraform-provider-docker") || download_url.contains("docker"),
            "Expected docker provider in URL, got: {}",
            download_url
        );
        assert!(
            download_url.contains("3.0.2"),
            "Expected version in URL, got: {}",
            download_url
        );
        assert!(
            download_url.ends_with(".zip"),
            "Expected .zip extension, got: {}",
            download_url
        );

        assert!(
            key.starts_with("registry.opentofu.org/kreuzwerker/docker/"),
            "Expected key to start with registry path, got: {}",
            key
        );
        assert_eq!(
            key.split('/').nth(1).unwrap(),
            "kreuzwerker",
            "Expected kreuzwerker namespace in key"
        );
    }

    #[tokio::test]
    async fn test_get_provider_url_key_docker_different_targets() {
        let tf_lock_provider = TfLockProvider {
            source: "registry.opentofu.org/kreuzwerker/docker".to_string(),
            version: "3.0.2".to_string(),
        };

        let result =
            get_provider_url_key(&tf_lock_provider, "darwin_amd64", "provider_binary").await;
        assert!(result.is_ok(), "Failed with darwin_amd64: {:?}", result);

        let result =
            get_provider_url_key(&tf_lock_provider, "darwin_arm64", "provider_binary").await;
        assert!(result.is_ok(), "Failed with darwin_arm64: {:?}", result);

        let result =
            get_provider_url_key(&tf_lock_provider, "linux_arm64", "provider_binary").await;
        assert!(result.is_ok(), "Failed with linux_arm64: {:?}", result);

        let result =
            get_provider_url_key(&tf_lock_provider, "windows_amd64", "provider_binary").await;
        assert!(result.is_ok(), "Failed with windows_amd64: {:?}", result);
    }

    #[tokio::test]
    async fn test_get_provider_url_key_invalid_target() {
        let tf_lock_provider = TfLockProvider {
            source: "registry.opentofu.org/hashicorp/aws".to_string(),
            version: "5.0.0".to_string(),
        };
        let target = "linux"; // Invalid - should be "linux_amd64"
        let category = "provider_binary";

        let result = get_provider_url_key(&tf_lock_provider, target, category).await;
        assert!(result.is_err(), "Expected error for invalid target format");

        let error_message = result.unwrap_err().to_string();
        assert!(
            error_message.contains("Invalid target format"),
            "Expected error message about invalid target format, got: {}",
            error_message
        );
    }

    #[tokio::test]
    async fn test_get_provider_url_key_invalid_category() {
        let tf_lock_provider = TfLockProvider {
            source: "registry.opentofu.org/hashicorp/aws".to_string(),
            version: "5.0.0".to_string(),
        };
        let target = "linux_amd64";
        let category = "invalid_category";

        let result = get_provider_url_key(&tf_lock_provider, target, category).await;
        assert!(result.is_err(), "Expected error for invalid category");

        let error_message = result.unwrap_err().to_string();
        assert!(
            error_message.contains("Invalid category"),
            "Expected error message about invalid category, got: {}",
            error_message
        );
    }

    #[tokio::test]
    async fn test_get_provider_url_key_nonexistent_version() {
        // Test with a version that doesn't exist (should fail at API level)
        let tf_lock_provider = TfLockProvider {
            source: "registry.opentofu.org/hashicorp/aws".to_string(),
            version: "999.999.999".to_string(),
        };
        let target = "linux_amd64";
        let category = "provider_binary";

        let result = get_provider_url_key(&tf_lock_provider, target, category).await;
        assert!(result.is_err(), "Expected error for nonexistent version");

        // The error should be about the API returning an error
        let error_message = result.unwrap_err().to_string();
        assert!(
            error_message.contains("Registry API") || error_message.contains("404"),
            "Expected API error message, got: {}",
            error_message
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_plan_get_destructive_changes_with_delete() {
        let plan_json = json!({
            "resource_changes": [{
                "address": "aws_s3_bucket.example",
                "change": {
                    "actions": ["delete"]
                }
            }]
        });
        let changes = plan_get_destructive_changes(&plan_json);
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].address, "aws_s3_bucket.example");
        assert_eq!(changes[0].action, "delete");
    }

    #[test]
    fn test_plan_get_destructive_changes_with_replace() {
        let plan_json = json!({
            "resource_changes": [{
                "address": "aws_instance.web",
                "change": {
                    "actions": ["delete", "create"]
                }
            }]
        });
        let changes = plan_get_destructive_changes(&plan_json);
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].address, "aws_instance.web");
        assert_eq!(changes[0].action, "replace");
    }

    #[test]
    fn test_plan_get_destructive_changes_mixed() {
        let plan_json = json!({
            "resource_changes": [
                {
                    "address": "aws_s3_bucket.old",
                    "change": {
                        "actions": ["delete"]
                    }
                },
                {
                    "address": "aws_instance.web",
                    "change": {
                        "actions": ["delete", "create"]
                    }
                },
                {
                    "address": "aws_s3_bucket.new",
                    "change": {
                        "actions": ["create"]
                    }
                }
            ]
        });
        let changes = plan_get_destructive_changes(&plan_json);
        assert_eq!(changes.len(), 2);
        assert_eq!(changes[0].address, "aws_s3_bucket.old");
        assert_eq!(changes[0].action, "delete");
        assert_eq!(changes[1].address, "aws_instance.web");
        assert_eq!(changes[1].action, "replace");
    }

    #[test]
    fn test_plan_get_destructive_changes_no_destructive() {
        let plan_json = json!({
            "resource_changes": [{
                "address": "aws_s3_bucket.new",
                "change": {
                    "actions": ["create"]
                }
            }]
        });
        let changes = plan_get_destructive_changes(&plan_json);
        assert_eq!(changes.len(), 0);
    }
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

pub fn get_extra_environment_variables_all(
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
    env_vars.insert(
        "INFRAWEAVE_DEPLOYMENT_ID".to_string(),
        deployment_id.to_string(),
    );
    env_vars.insert(
        "INFRAWEAVE_ENVIRONMENT".to_string(),
        environment.to_string(),
    );
    env_vars.insert("INFRAWEAVE_REFERENCE".to_string(), reference.to_string());
    env_vars.insert(
        "INFRAWEAVE_MODULE_VERSION".to_string(),
        module_version.to_string(),
    );
    env_vars.insert(
        "INFRAWEAVE_MODULE_TYPE".to_string(),
        module_type.to_string(),
    );
    env_vars.insert(
        "INFRAWEAVE_MODULE_TRACK".to_string(),
        module_track.to_string(),
    );
    env_vars.insert(
        "INFRAWEAVE_DRIFT_DETECTION".to_string(),
        (if drift_detection.enabled {
            "enabled"
        } else {
            "disabled"
        })
        .to_string(),
    );
    env_vars.insert(
        "INFRAWEAVE_DRIFT_DETECTION_INTERVAL".to_string(),
        if drift_detection.enabled {
            drift_detection.interval.to_string()
        } else {
            "N/A".to_string()
        },
    );

    match &extra_data {
        ExtraData::GitHub(github_data) => {
            env_vars.insert(
                "INFRAWEAVE_GIT_COMMITTER_EMAIL".to_string(),
                github_data.user.email.clone(),
            );
            env_vars.insert(
                "INFRAWEAVE_GIT_COMMITTER_NAME".to_string(),
                github_data.user.name.clone(),
            );
            env_vars.insert(
                "INFRAWEAVE_GIT_ACTOR_USERNAME".to_string(),
                github_data.user.username.clone(),
            );
            env_vars.insert(
                "INFRAWEAVE_GIT_ACTOR_PROFILE_URL".to_string(),
                github_data.user.profile_url.clone(),
            );
            env_vars.insert(
                "INFRAWEAVE_GIT_REPOSITORY_NAME".to_string(),
                github_data.repository.full_name.clone(),
            );
            env_vars.insert(
                "INFRAWEAVE_GIT_REPOSITORY_PATH".to_string(),
                github_data.job_details.file_path.clone(),
            );
            env_vars.insert(
                "INFRAWEAVE_GIT_COMMIT_SHA".to_string(),
                github_data.check_run.head_sha.clone(),
            );
        }
        ExtraData::GitLab(gitlab_data) => {
            // TODO: Add more here for GitLab
            env_vars.insert(
                "INFRAWEAVE_GIT_REPOSITORY_PATH".to_string(),
                gitlab_data.job_details.file_path.clone(),
            );
        }
        ExtraData::None => {}
    };
    env_vars
}
