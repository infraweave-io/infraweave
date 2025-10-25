use anyhow::{Context, Result};
use env_defs::TfLockProvider;
use serde::Deserialize;
use serde_json::Value;

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
///   - registry.terraform.io
///   - custom-registry.company.com
pub async fn get_provider_url_key(
    tf_lock_provider: &TfLockProvider,
    target: &str,
    category: &str,
) -> Result<(String, String)> {
    let parts: Vec<&str> = tf_lock_provider.source.split('/').collect();
    // parts: ["registry.terraform.io", "hashicorp", "aws"]
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

    let key = format!("registry.terraform.io/{}/{}/{}", namespace, provider, file);
    Ok((download_url, key))
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
            source: "registry.terraform.io/hashicorp/aws".to_string(),
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
            key.starts_with("registry.terraform.io/hashicorp/aws/"),
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
            source: "registry.terraform.io/hashicorp/aws".to_string(),
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
            source: "registry.terraform.io/hashicorp/aws".to_string(),
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
            source: "registry.terraform.io/kreuzwerker/docker".to_string(),
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
            key.starts_with("registry.terraform.io/kreuzwerker/docker/"),
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
            source: "registry.terraform.io/kreuzwerker/docker".to_string(),
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
            source: "registry.terraform.io/hashicorp/aws".to_string(),
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
            source: "registry.terraform.io/hashicorp/aws".to_string(),
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
            source: "registry.terraform.io/hashicorp/aws".to_string(),
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
