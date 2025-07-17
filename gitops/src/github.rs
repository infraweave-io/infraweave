use base64::decode;
use chrono::{DateTime, Utc};
use env_common::interface::GenericCloudHandler;
use env_common::logic::{
    get_deployment_details, publish_module_from_zip, publish_notification, run_claim,
    set_deployment,
};
use env_defs::{ArtifactType, CloudProvider, ModuleResp, OciArtifactSet};
use env_defs::{
    CheckRun, CheckRunOutput, DeploymentManifest, ExtraData, GitHubCheckRun, Installation,
    JobDetails, NotificationData, Owner, Repository, User,
};
use env_utils::{
    convert_module_example_variables_to_snake_case, get_module_manifest_from_oci_targz,
    get_module_zip_from_oci_targz,
};
use futures::stream::{self, StreamExt};
use hmac::{Hmac, Mac};
use jsonwebtoken::{encode, EncodingKey, Header};
use log::info;
use regex::Regex;
use reqwest::blocking::Client;
use reqwest::header;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::Sha256;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};
use std::{env, error::Error};
use subtle::ConstantTimeEq;

use crate::{
    get_project_id_for_repository_path, get_securestring_aws, group_files_by_manifest, FileChange,
    ProcessedFiles,
};

const INFRAWEAVE_USER_AGENT: &str = "infraweave/gitops";
const GITHUB_API_URL: &str = "https://api.github.com";

// Create an alias for HMAC-SHA256.
type HmacSha256 = Hmac<Sha256>;

/// Direct representation of GitHub package webhook payload structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitHubPackageWebhook {
    pub action: String,
    #[serde(rename = "registry_package")]
    pub registry_package: Option<GitHubPackage>,
    #[serde(rename = "package")]
    pub package: Option<GitHubPackage>,
    pub repository: GitHubRepository,
    pub installation: Option<GitHubInstallation>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitHubPackage {
    pub name: String,
    pub package_type: String,
    pub owner: GitHubOwner,
    pub package_version: GitHubPackageVersion,
    pub registry: Option<GitHubRegistry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitHubPackageVersion {
    pub version: String,
    pub package_url: Option<String>,
    pub container_metadata: Option<GitHubContainerMetadata>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitHubContainerMetadata {
    pub tags: Option<Vec<String>>,
    pub tag: Option<GitHubTag>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitHubTag {
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitHubRegistry {
    pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitHubRepository {
    pub full_name: String,
    pub name: String,
    pub html_url: String,
    pub owner: GitHubOwner,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitHubOwner {
    pub login: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitHubInstallation {
    pub id: u64,
}

/// Detects artifact type and tag from a deserialized webhook structure
fn detect_artifact_type_and_tag_from_webhook(
    webhook: &GitHubPackageWebhook,
) -> (ArtifactType, String) {
    // Get package information - try registry_package first, then package
    let package_info = webhook
        .registry_package
        .as_ref()
        .or(webhook.package.as_ref());

    let package_name = package_info.map(|p| p.name.as_str()).unwrap_or("");

    let package_version = package_info
        .map(|p| p.package_version.version.as_str())
        .unwrap_or("");

    println!(
        "🔍 Analyzing package for artifact type - Name: '{}', Version: '{}'",
        package_name, package_version
    );

    // Check for attestation indicators in name or version
    if package_name.ends_with(".att") || package_name.contains("attestation") {
        println!(
            "📋 Detected attestation artifact based on package name: {}",
            package_name
        );
        return (ArtifactType::Attestation, package_version.to_string());
    }

    if package_version.ends_with(".att") || package_version.contains("attestation") {
        println!(
            "📋 Detected attestation artifact based on version: {}",
            package_version
        );
        return (ArtifactType::Attestation, package_version.to_string());
    }

    // Check for signature indicators
    if package_name.ends_with(".sig") || package_name.contains("signature") {
        println!(
            "✍️ Detected signature artifact based on package name: {}",
            package_name
        );
        return (ArtifactType::Signature, package_version.to_string());
    }

    if package_version.ends_with(".sig") || package_version.contains("signature") {
        println!(
            "✍️ Detected signature artifact based on version: {}",
            package_version
        );
        return (ArtifactType::Signature, package_version.to_string());
    }

    // Check container metadata if available
    if let Some(package) = package_info {
        if let Some(container_metadata) = &package.package_version.container_metadata {
            // Check tags array
            if let Some(tags) = &container_metadata.tags {
                if let Some(tag) = tags.iter().next() {
                    if tag.ends_with(".att") || tag.contains("attestation") {
                        println!(
                            "📋 Detected attestation artifact based on container metadata tag: {}",
                            tag
                        );
                        return (ArtifactType::Attestation, tag.clone());
                    }
                    if tag.ends_with(".sig") || tag.contains("signature") {
                        println!(
                            "✍️ Detected signature artifact based on container metadata tag: {}",
                            tag
                        );
                        return (ArtifactType::Signature, tag.clone());
                    }
                    println!("📦 Detected main package artifact with tag: {}", tag);
                    return (ArtifactType::MainPackage, tag.clone());
                }
            }

            // Check single tag object
            if let Some(tag_obj) = &container_metadata.tag {
                let tag_name = &tag_obj.name;
                if tag_name.ends_with(".att") || tag_name.contains("attestation") {
                    println!(
                        "📋 Detected attestation artifact based on container metadata tag.name: {}",
                        tag_name
                    );
                    return (ArtifactType::Attestation, tag_name.clone());
                }
                if tag_name.ends_with(".sig") || tag_name.contains("signature") {
                    println!(
                        "✍️ Detected signature artifact based on container metadata tag.name: {}",
                        tag_name
                    );
                    return (ArtifactType::Signature, tag_name.clone());
                }
                println!(
                    "📦 Detected main package artifact with tag.name: {}",
                    tag_name
                );
                return (ArtifactType::MainPackage, tag_name.clone());
            }
        }
    }

    // Default to main package with package version as tag
    println!("📦 No specific artifact type indicators found, assuming main package");
    println!("🔄 Using package version as tag: {}", package_version);
    (ArtifactType::MainPackage, package_version.to_string())
}

#[derive(Debug, Deserialize)]
struct FileContent {
    content: String,
    encoding: String,
}

#[derive(Debug, Deserialize)]
struct WebhookPayload {
    #[serde(rename = "ref")]
    _ref: String, // branch name
    before: String, // commit SHA before the push
    after: String,  // commit SHA after the push
    commits: Vec<Commit>,
}

#[derive(Debug, Deserialize)]
struct Commit {
    // id: String,
    // tree_id: String,
    added: Vec<String>,
    removed: Vec<String>,
    modified: Vec<String>,
}

fn get_default_branch(owner: &str, repo: &str, token: &str) -> Result<String, Box<dyn Error>> {
    let client = Client::new();

    let repo_url = format!("{}/repos/{}/{}", GITHUB_API_URL, owner, repo);
    let repo_response = client
        .get(&repo_url)
        .header("User-Agent", INFRAWEAVE_USER_AGENT)
        .header("Authorization", format!("token {}", token))
        .send()?;
    let repo_info: Value = repo_response.error_for_status()?.json()?;
    let default_branch = repo_info["default_branch"]
        .as_str()
        .ok_or("Missing default_branch in repository info")?;

    Ok(default_branch.to_string())
}

fn get_default_branch_sha(owner: &str, repo: &str, token: &str) -> Result<String, Box<dyn Error>> {
    let client = Client::new();

    let default_branch = get_default_branch(owner, repo, token)?;

    // Fetch the latest commit for the default branch.
    let commit_url = format!(
        "{}/repos/{}/{}/commits/{}",
        GITHUB_API_URL, owner, repo, default_branch
    );
    let commit_response = client
        .get(&commit_url)
        .header("User-Agent", INFRAWEAVE_USER_AGENT)
        .header("Authorization", format!("token {}", token))
        .send()?;
    let commit_info: Value = commit_response.error_for_status()?.json()?;
    let sha = commit_info["sha"]
        .as_str()
        .ok_or("Missing sha in commit info")?
        .to_string();

    Ok(sha)
}

/// Fetch file content from GitHub for a commit reference
/// If a 404 is returned, we treat that as "None" (file does not exist)
fn get_file_content_option(
    owner: &str,
    repo: &str,
    path: &str,
    reference: &str,
    token: &str,
) -> Result<Option<String>, Box<dyn Error>> {
    let url = format!(
        "{}/repos/{}/{}/contents/{}?ref={}",
        GITHUB_API_URL, owner, repo, path, reference
    );
    let client = Client::new();
    let response = client
        .get(&url)
        .header("User-Agent", INFRAWEAVE_USER_AGENT)
        .header("Authorization", format!("token {}", token))
        .send()?;

    if response.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(None);
    }

    let resp = response.error_for_status()?;
    let file: FileContent = resp.json()?;
    if file.encoding != "base64" {
        return Err("Unexpected encoding".into());
    }
    let decoded_bytes = decode(file.content.replace("\n", ""))?;
    let content = String::from_utf8(decoded_bytes)?;
    Ok(Some(content))
}

fn process_webhook_files(
    owner: &str,
    repo: &str,
    token: &str,
    payload: &WebhookPayload,
) -> Result<ProcessedFiles, Box<dyn Error>> {
    let default_branch = get_default_branch(owner, repo, token)?;
    let current_branch = payload
        ._ref
        .strip_prefix("refs/heads/")
        .unwrap_or(&payload._ref);
    let before_ref = if current_branch == default_branch {
        // For main, compare with the previous commit.
        payload.before.clone()
    } else {
        // For other branches, get the current commit SHA on main.
        get_default_branch_sha(owner, repo, token)?
    };
    let after_ref = &payload.after;

    let mut added = std::collections::HashSet::new();
    let mut removed = std::collections::HashSet::new();
    let mut modified = std::collections::HashSet::new();
    for commit in &payload.commits {
        for file in &commit.added {
            added.insert(file.clone());
        }
        for file in &commit.removed {
            removed.insert(file.clone());
        }
        for file in &commit.modified {
            modified.insert(file.clone());
        }
    }
    let mut all_files = std::collections::HashSet::new();
    all_files.extend(added.iter().cloned());
    all_files.extend(removed.iter().cloned());
    all_files.extend(modified.iter().cloned());

    let mut active_files = Vec::new();
    let mut deleted_files = Vec::new();

    for file in all_files {
        if modified.contains(&file) {
            // For modified files, fetch both before and after.
            let active_content = get_file_content_option(owner, repo, &file, after_ref, token)?
                .unwrap_or_else(String::new);
            let deleted_content = get_file_content_option(owner, repo, &file, &before_ref, token)?
                .unwrap_or_else(String::new);
            active_files.push(FileChange {
                path: file.clone(),
                content: active_content,
            });
            deleted_files.push(FileChange {
                path: file.clone(),
                content: deleted_content,
            });
        } else if added.contains(&file) {
            // For added files, only after.
            if let Some(active_content) =
                get_file_content_option(owner, repo, &file, after_ref, token)?
            {
                active_files.push(FileChange {
                    path: file.clone(),
                    content: active_content,
                });
            }
        } else if removed.contains(&file) {
            // For removed files, only before.
            if let Some(deleted_content) =
                get_file_content_option(owner, repo, &file, &before_ref, token)?
            {
                deleted_files.push(FileChange {
                    path: file.clone(),
                    content: deleted_content,
                });
            }
        }
    }
    Ok(ProcessedFiles {
        active_files,
        deleted_files,
    })
}

fn verify_signature(payload_body: &[u8], signature: &str, github_secret: &str) -> bool {
    if signature.is_empty() {
        return false;
    }

    let mut mac = HmacSha256::new_from_slice(github_secret.as_bytes())
        .expect("HMAC can take key of any size");
    mac.update(payload_body);
    let result = mac.finalize();
    let expected_bytes = result.into_bytes();

    let expected_hex = hex::encode(expected_bytes);
    let computed_sig = format!("sha256={}", expected_hex);

    // Compare using constant-time equality check to prevent timing attacks.
    computed_sig
        .as_bytes()
        .ct_eq(signature.as_bytes())
        .unwrap_u8()
        == 1
}

/// Claims for the GitHub App JWT.
#[derive(Debug, Serialize, Deserialize)]
struct Claims {
    iat: usize,  // Issued at time (seconds since epoch)
    exp: usize,  // Expiration time (seconds since epoch)
    iss: String, // GitHub App ID
}

/// The response from GitHub when requesting an installation access token.
#[derive(Debug, Deserialize)]
struct InstallationTokenResponse {
    token: String,
    // expires_at: String,
}

fn get_installation_token(
    installation_id: u64,
    app_id: &str,
    private_key_pem: &str,
) -> Result<String, Box<dyn Error>> {
    get_installation_token_with_permissions(installation_id, app_id, private_key_pem, None)
}

fn get_installation_token_with_permissions(
    installation_id: u64,
    app_id: &str,
    private_key_pem: &str,
    permissions: Option<Value>,
) -> Result<String, Box<dyn Error>> {
    // Generate a JWT valid for 10 minutes. Allow a 60-second clock skew.
    let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
    let iat = now - 60;
    let exp = now + 10 * 60;
    let claims = Claims {
        iat: iat as usize,
        exp: exp as usize,
        iss: app_id.to_owned(),
    };
    let header = Header::new(jsonwebtoken::Algorithm::RS256);
    let jwt = encode(
        &header,
        &claims,
        &EncodingKey::from_rsa_pem(private_key_pem.as_bytes())?,
    )?;

    let url = format!(
        "{}/app/installations/{}/access_tokens",
        GITHUB_API_URL, installation_id
    );

    let client = Client::new();

    let mut request_body = json!({});

    // Only add permissions if explicitly requested
    if let Some(perms) = permissions {
        request_body["permissions"] = perms;
    }

    println!(
        "🔧 Requesting GitHub App token with request body: {}",
        serde_json::to_string_pretty(&request_body).unwrap_or_default()
    );

    let response = client
        .post(&url)
        .header("Authorization", format!("Bearer {}", jwt))
        .header("Accept", "application/vnd.github+json")
        .header("User-Agent", INFRAWEAVE_USER_AGENT)
        .json(&request_body)
        .send()?;

    let status = response.status();
    println!("🔧 GitHub App token request status: {}", status);

    if !status.is_success() {
        let error_text = response.text()?;
        println!("❌ GitHub App token request failed: {}", error_text);
        return Err(format!(
            "GitHub App token request failed with status {}: {}",
            status, error_text
        )
        .into());
    }

    let response_text = response.text()?;
    let token_response: InstallationTokenResponse = serde_json::from_str(&response_text)?;
    Ok(token_response.token)
}

pub async fn handle_validate_github_event(event: &Value) -> Result<Value, anyhow::Error> {
    println!("Event: {:?}", event);

    let body_str = event.get("body").and_then(|b| b.as_str()).unwrap_or("");
    let body = body_str.as_bytes();

    let empty_map = serde_json::Map::new();
    let headers = event
        .get("headers")
        .and_then(|h| h.as_object())
        .unwrap_or(&empty_map);
    let signature = headers
        .get("x-hub-signature-256")
        .and_then(|s| s.as_str())
        .unwrap_or("");

    let github_secret_parameter_store_key = env::var("GITHUB_SECRET_PARAMETER_STORE_KEY")
        .expect("GITHUB_SECRET_PARAMETER_STORE_KEY environment variable not set");
    let github_secret = get_securestring_aws(&github_secret_parameter_store_key).await?;

    if !verify_signature(body, signature, &github_secret) {
        return Err(anyhow::anyhow!("Invalid signature"));
    }

    let handler = GenericCloudHandler::default().await;

    let notification = NotificationData {
        subject: "validated_github_event".to_string(),
        message: event.clone(),
    };

    return match publish_notification(&handler, notification).await {
        Ok(_) => {
            println!("Notification published");
            Ok(json!({
                "statusCode": 200,
                "body": "Validated successfully and forwarded for processing",
            }))
        }
        Err(e) => {
            println!("Error publishing notification: {:?}", e);
            Err(anyhow::anyhow!("Error publishing notification: {:?}", e))
        }
    };
}

pub async fn handle_process_push_event(event: &Value) -> Result<Value, anyhow::Error> {
    println!("handle_process_push_event: {:?}", event);
    let body_str = event.get("body").and_then(|b| b.as_str()).unwrap_or("");
    let body = body_str.as_bytes();

    let empty_map = serde_json::Map::new();
    let headers = event
        .get("headers")
        .and_then(|h| h.as_object())
        .unwrap_or(&empty_map);

    let payload: Value = serde_json::from_slice(body).expect("Failed to parse JSON payload");

    let branch = payload["ref"].as_str().unwrap();
    println!("Branch: {}", branch);

    let installation_id = payload["installation"]["id"].as_u64().unwrap();
    let app_id = headers
        .get("x-github-hook-installation-target-id")
        .and_then(|s| s.as_str())
        .unwrap_or("");

    let owner = payload["repository"]["owner"]["login"].as_str().unwrap();
    let repo = payload["repository"]["name"].as_str().unwrap();
    let repo_full_name = payload["repository"]["full_name"].as_str().unwrap();
    let repository_url = payload["repository"]["html_url"].as_str().unwrap();
    let author_name = match payload["head_commit"]["author"]["name"].as_str() {
        Some(name) => name,
        None => {
            println!("Author name not found in payload: {:?}", payload);
            "Unknown author name"
        }
    };
    let author_email = match payload["head_commit"]["author"]["email"].as_str() {
        Some(email) => email,
        None => {
            println!("Author email not found in payload: {:?}", payload);
            "Unknown author email"
        }
    };
    let sender_login = payload["sender"]["login"].as_str().unwrap();
    let sender_profile_url = payload["sender"]["html_url"].as_str().unwrap();

    let (project_id, project_id_found) =
        match get_project_id_for_repository_path(repo_full_name).await {
            Ok(project_id) => (project_id, true),
            Err(e) => {
                println!("Error getting project id: {:?}", e);
                ("NOT_FOUND_FOR_REPO".to_string(), false)
            }
        };

    // Can save money on SSM API calls if we know 'project_id_found' is 'false' by returning early.
    // However it is important to inform the user that the project_id is missing.
    // Hence we will still process the files and inform the user in the end.

    let private_key_pem_ssm_key = env::var("GITHUB_PRIVATE_KEY_PARAMETER_STORE_KEY")
        .expect("GITHUB_PRIVATE_KEY_PARAMETER_STORE_KEY environment variable not set");
    let private_key_pem = get_securestring_aws(&private_key_pem_ssm_key).await?; // Read here to avoid multiple reads of the same secret
    let token = get_installation_token(installation_id, app_id, &private_key_pem).unwrap();

    let payload: WebhookPayload = serde_json::from_str(body_str).unwrap();

    let processed = process_webhook_files(owner, repo, &token, &payload).unwrap();
    println!("Processed files: {:?}", processed);

    let grouped = group_files_by_manifest(processed);
    println!("Grouped files: {:?}", grouped);

    println!(
        "Found project id: {} for path: {}",
        project_id, repo_full_name
    );

    let default_branch = get_default_branch(owner, repo, &token).unwrap_or("main".to_string());

    stream::iter(grouped)
        .for_each_concurrent(None, |group| {
            // TODO: make smaller functions of below code
            let payload = &payload;
            let default_branch = &default_branch;
            let private_key_pem = &private_key_pem;
            let project_id = &project_id;
            async move {
                let mut extra_data = ExtraData::GitHub(GitHubCheckRun {
                    installation: Installation {
                        id: installation_id,
                    },
                    app_id: app_id.to_string(),
                    repository: Repository {
                        owner: Owner {
                            login: owner.to_string(),
                        },
                        name: repo.to_string(),
                        full_name: repo_full_name.to_string(),
                    },
                    check_run: CheckRun {
                        head_sha: payload.after.clone(),
                        status: "in_progress".to_string(),
                        name: "OVERRIDE".to_string(),
                        started_at: Some(Utc::now().to_rfc3339()),
                        completed_at: None,
                        conclusion: None,
                        details_url: None,
                        output: None,
                    },
                    job_details: JobDetails {
                        region: "OVERRIDE".to_string(),
                        environment: "OVERRIDE".to_string(),
                        deployment_id: "OVERRIDE".to_string(),
                        job_id: "OVERRIDE".to_string(),
                        change_type: "OVERRIDE".to_string(),
                        file_path: "OVERRIDE".to_string(),
                        manifest_yaml: "OVERRIDE".to_string(),
                        error_text: "OVERRIDE".to_string(),
                        status: "OVERRIDE".to_string(),
                    },
                    user: User {
                        email: author_email.to_string(),
                        name: author_name.to_string(),
                        username: sender_login.to_string(),
                        profile_url: sender_profile_url.to_string(),
                    },
                });
                if let Some((active, canonical)) = group.active {
                    if !project_id_found {
                        inform_missing_project_configuration(
                            &mut extra_data,
                            active.path.as_str(),
                            private_key_pem,
                        )
                        .await;
                        return; // Exit early if project id is not found
                    }
                    println!("Apply job for: {:?} from path: {}", group.key, active.path);
                    let yaml = serde_yaml::from_str::<serde_yaml::Value>(&canonical).unwrap();
                    println!("YAML: {:?}", yaml);
                    let command = if branch != format!("refs/heads/{}", default_branch) {
                        "plan"
                    } else {
                        "apply"
                    };
                    if let ExtraData::GitHub(ref mut github_check_run) = extra_data {
                        github_check_run.job_details.file_path = active.path.clone();
                        github_check_run.job_details.manifest_yaml =
                            canonical.clone().trim_start_matches("---").to_string();
                        let region = if let Some(region) = yaml["spec"]["region"].as_str() {
                            region.to_string()
                        } else {
                            "unknown_region".to_string()
                        };
                        if let Some(new_name) = yaml["metadata"]["name"].as_str() {
                            let namespace = if let Some(env) = yaml["metadata"]["namespace"].as_str() {
                                env.to_string()
                            } else {
                                "default".to_string()
                            };
                            github_check_run.check_run.name =
                                get_check_run_name(new_name, &active.path, &region, &namespace);
                        }
                        github_check_run.check_run.output = Some(CheckRunOutput {
                            title: format!("{} job initiated", command),
                            summary: format!(
                                "Running {} job for applying resources for {}, please wait...",
                                command, github_check_run.check_run.name
                            ),
                            text: Some(format!(
                                r#"
## Claim

```yaml
{}
```"#,
                                canonical.trim_start_matches("---")
                            )),
                            annotations: None,
                        });
                    }
                    match serde_yaml::from_value::<DeploymentManifest>(yaml.clone()) {
                        Ok(deployment_claim) => {
                            let region = &deployment_claim.spec.region;
                            let handler = GenericCloudHandler::workload(project_id, region).await;
                            let flags = vec![];
                            let full_file_url = format!(
                                "{}/blob/{}/{}",
                                repository_url, default_branch, active.path
                            );
                            let namespace = deployment_claim
                                .metadata
                                .namespace
                                .unwrap_or("default".to_string());
                            // Prevent collision between repos by using repo_full_name
                            let repo_full_name_dash = repo_full_name.replace("/", "-").to_lowercase();
                            match run_claim(
                                &handler,
                                &yaml,
                                &format!("github-{}/{}", repo_full_name_dash, namespace),
                                command,
                                flags,
                                extra_data.clone(),
                                &full_file_url,
                            )
                            .await
                            {
                                Ok(_) => {
                                    println!("Apply job completed");
                                }
                                Err(e) => {
                                    println!("Apply job failed: {:?}", e);
                                    if let ExtraData::GitHub(ref mut github_check_run) = extra_data
                                    {
                                        github_check_run.check_run.status = "completed".to_string();
                                        github_check_run.check_run.conclusion =
                                            Some("failure".to_string());
                                        github_check_run.check_run.completed_at =
                                            Some(Utc::now().to_rfc3339());
                                        github_check_run.check_run.output = Some(CheckRunOutput {
                                            title: "Apply job failed".into(),
                                            summary: format!(
                                                "Failed to apply resources for {}",
                                                github_check_run.check_run.name
                                            ),
                                            text: Some(format!("Error: {}", e)),
                                            annotations: None,
                                        });
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            println!("Error parsing deployment manifest: {:?}", e);
                            println!("Apply job failed: {:?}", e);
                            if let ExtraData::GitHub(ref mut github_check_run) = extra_data {
                                github_check_run.check_run.status = "completed".to_string();
                                github_check_run.check_run.conclusion = Some("failure".to_string());
                                github_check_run.check_run.completed_at =
                                    Some(Utc::now().to_rfc3339());
                                github_check_run.check_run.output = Some(CheckRunOutput {
                                    title: "Apply job failed".into(),
                                    summary: format!(
                                        "Failed to apply resources for {}",
                                        github_check_run.check_run.name
                                    ),
                                    text: Some(format!("Error: {}", e)),
                                    annotations: None,
                                });
                            }
                        }
                    };
                } else if let Some((deleted, canonical)) = group.deleted {
                    if !project_id_found {
                        inform_missing_project_configuration(
                            &mut extra_data,
                            deleted.path.as_str(),
                            private_key_pem,
                        )
                        .await;
                        return; // Exit early if project id is not found
                    }
                    println!(
                        "Destroy job for: {:?} from path: {}",
                        group.key, deleted.path
                    );
                    let yaml = serde_yaml::from_str::<serde_yaml::Value>(&canonical).unwrap();
                    println!("YAML: {:?}", yaml);
                    let command = if branch != format!("refs/heads/{}", default_branch) {
                        "plan"
                    } else {
                        "destroy"
                    };
                    if let ExtraData::GitHub(ref mut github_check_run) = extra_data {
                        github_check_run.job_details.file_path = deleted.path.clone();
                        github_check_run.job_details.manifest_yaml =
                            canonical.clone().trim_start_matches("---").to_string();
                        let region = if let Some(region) = yaml["spec"]["region"].as_str() {
                            region.to_string()
                        } else {
                            "unknown_region".to_string()
                        };
                        if let Some(new_name) = yaml["metadata"]["name"].as_str() {
                            let namespace = if let Some(env) = yaml["metadata"]["namespace"].as_str() {
                                env.to_string()
                            } else {
                                "default".to_string()
                            };
                            github_check_run.check_run.name =
                                get_check_run_name(new_name, &deleted.path, &region, &namespace);
                        }
                        github_check_run.check_run.output = Some(CheckRunOutput {
                            title: format!("{} job initiated", command),
                            summary: format!(
                                "Running {} job for deleting resources for {}, please wait...",
                                command, github_check_run.check_run.name
                            ),
                            text: Some(format!(
                                r#"
## Claim

```yaml
{}
```"#,
                                canonical.trim_start_matches("---")
                            )),
                            annotations: None,
                        });
                    }
                    match serde_yaml::from_value::<DeploymentManifest>(yaml.clone()) {
                        Ok(deployment_claim) => {
                            let region = &deployment_claim.spec.region;
                            let handler = GenericCloudHandler::workload(project_id, region).await;
                            let flags = if command == "plan" {
                                vec!["-destroy".to_string()]
                            } else {
                                vec![]
                            };
                            let full_file_url = format!(
                                "{}/blob/{}/{}",
                                repository_url, default_branch, deleted.path
                            );
                            let namespace = deployment_claim
                                .metadata
                                .namespace
                                .unwrap_or("default".to_string());
                            // Prevent collision between repos by using repo_full_name
                            let repo_full_name_dash = repo_full_name.replace("/", "-").to_lowercase();
                            match run_claim(
                                &handler,
                                &yaml,
                                &format!("github-{}/{}", repo_full_name_dash, namespace),
                                command,
                                flags,
                                extra_data.clone(),
                                &full_file_url,
                            )
                            .await
                            {
                                Ok(_) => {
                                    println!("Destroy job completed");
                                }
                                Err(e) => {
                                    println!("Destroy job failed: {:?}", e);
                                    if let ExtraData::GitHub(ref mut github_check_run) = extra_data
                                    {
                                        github_check_run.check_run.status = "completed".to_string();
                                        github_check_run.check_run.conclusion =
                                            Some("failure".to_string());
                                        github_check_run.check_run.completed_at =
                                            Some(Utc::now().to_rfc3339());
                                        github_check_run.check_run.output = Some(CheckRunOutput {
                                            title: "Destroy job failed".into(),
                                            summary: format!(
                                                "Failed to destroy resources for {}",
                                                github_check_run.check_run.name
                                            ),
                                            text: Some(format!("Error: {}", e)),
                                            annotations: None,
                                        });
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            println!("Error parsing deployment manifest: {:?}", e);
                            println!("Destroy job failed: {:?}", e);
                            if let ExtraData::GitHub(ref mut github_check_run) = extra_data {
                                github_check_run.check_run.status = "completed".to_string();
                                github_check_run.check_run.conclusion = Some("failure".to_string());
                                github_check_run.check_run.completed_at =
                                    Some(Utc::now().to_rfc3339());
                                github_check_run.check_run.output = Some(CheckRunOutput {
                                    title: "Destroy job failed".into(),
                                    summary: format!(
                                        "Failed to destroy resources for {}",
                                        github_check_run.check_run.name
                                    ),
                                    text: Some(format!("Error: {}", e)),
                                    annotations: None,
                                });
                            }
                        }
                    };
                } else if let Some((renamed, canonical)) = group.renamed {
                    if !project_id_found {
                        inform_missing_project_configuration(
                            &mut extra_data,
                            renamed.path.as_str(),
                            private_key_pem,
                        )
                        .await;
                        return; // Exit early if project id is not found
                    }
                    let yaml = serde_yaml::from_str::<serde_yaml::Value>(&canonical).unwrap();
                    println!(
                        "Rename job for: {:?} from path: {}",
                        group.key, renamed.path
                    );
                    match serde_yaml::from_value::<DeploymentManifest>(yaml.clone()) {
                        Ok(deployment_claim) => {
                            let region = &deployment_claim.spec.region;
                            let handler = GenericCloudHandler::workload(project_id, region).await;
                            let full_file_url = format!(
                                "{}/blob/{}/{}",
                                repository_url, default_branch, renamed.path
                            );

                            let namespace = deployment_claim.clone()
                                .metadata
                                .namespace
                                .unwrap_or("default".to_string());

                                let repo_full_name_dash = repo_full_name.replace("/", "-");
                            let environment = &format!("github-{}/{}", repo_full_name_dash, namespace);
                            let (_region, environment, deployment_id, _module, name) =
                                get_deployment_details(environment, deployment_claim.clone()).unwrap();

                            let mut deployment = handler
                                .get_deployment(
                                    &deployment_id,
                                    &environment,
                                    false,
                                )
                                .await
                                .unwrap().unwrap();

                            if let ExtraData::GitHub(ref mut github_check_run) = extra_data {
                                deployment.reference = full_file_url;
                                set_deployment(&handler, &deployment, false).await.unwrap();

                                github_check_run.check_run.name =
                                    get_check_run_name(&name, &renamed.path, region, &namespace);
                                github_check_run.check_run.status = "completed".to_string();
                                github_check_run.check_run.conclusion = Some("success".to_string());
                                github_check_run.check_run.completed_at =
                                    Some(Utc::now().to_rfc3339());
                                github_check_run.check_run.output = Some(CheckRunOutput {
                                    title: "Renamed file".into(),
                                    summary: format!(
                                        "File `{}` has been renamed; the reference has been updated for `{}`.",
                                        renamed.path, &deployment_id
                                    ),
                                    text: Some(
                                        "No run has been triggered, only the reference has been updated.".to_string()
                                    ),
                                    annotations: None,
                                });
                            }
                        }
                        Err(e) => {
                            println!("Error parsing deployment manifest: {:?}", e);
                            println!("Rename job failed: {:?}", e);
                            if let ExtraData::GitHub(ref mut github_check_run) = extra_data {
                                github_check_run.check_run.status = "completed".to_string();
                                github_check_run.check_run.conclusion = Some("failure".to_string());
                                github_check_run.check_run.completed_at =
                                    Some(Utc::now().to_rfc3339());
                                github_check_run.check_run.output = Some(CheckRunOutput {
                                    title: "Rename job failed".into(),
                                    summary: format!(
                                        "Failed to rename resources for {:?}",
                                        group.key
                                    ),
                                    text: Some(format!("Error: {}", e)),
                                    annotations: None,
                                });
                            }
                        }
                    }
                } else {
                    println!("Group with key {:?} has no file!", group.key);
                }
                if let ExtraData::GitHub(github_check_run) = extra_data {
                    post_check_run_from_payload(github_check_run, private_key_pem)
                        .await
                        .unwrap();
                }
            }
        })
        .await;

    Ok(json!({
        "statusCode": 200,
        "body": "Processed successfully",
    }))
}

pub async fn handle_check_run_event(event: &Value) -> Result<Value, anyhow::Error> {
    let body_str = event.get("body").and_then(|b| b.as_str()).unwrap_or("");
    let payload: Value = serde_json::from_str(body_str).expect("Failed to parse JSON payload");
    let headers: Value = event.get("headers").unwrap_or(&json!({})).clone();

    match payload["action"].as_str() {
        Some("rerequested") => handle_check_run_rerequested_event(&payload, &headers).await,
        // TODO: Add more check_run actions
        _ => Err(anyhow::anyhow!("Invalid action {}", payload["action"])),
    }
}

#[derive(Debug, Deserialize)]
pub struct Package {
    pub id: u64,
    pub name: String,
    #[serde(rename = "package_type")]
    pub package_type: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub html_url: String,
}

#[derive(Debug, Deserialize)]
pub struct PackageVersion {
    pub id: u64,
    pub name: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub metadata: serde_json::Value,
    pub html_url: String,
}

#[derive(Debug)]
pub struct PackageWithVersions {
    pub package: Package,
    pub recent_versions: Vec<PackageVersion>,
}

/// Example pattern: GITHUB_PACKAGE_FILTER_PATTERNS="^(module|stack)-.*"
fn is_infraweave_package(package_name: &str) -> bool {
    let pattern = match std::env::var("GITHUB_PACKAGE_FILTER_PATTERNS") {
        Ok(env_pattern) => env_pattern.trim().to_string(),
        Err(_) => {
            println!("⚠️  GITHUB_PACKAGE_FILTER_PATTERNS environment variable not set. All packages will be skipped (none will pass filtering).");
            return false;
        }
    };

    // Compile and match the single regex pattern
    match Regex::new(&pattern) {
        Ok(regex) => regex.is_match(package_name),
        Err(e) => {
            println!(
                "⚠️  Invalid regex pattern '{}' in GITHUB_PACKAGE_FILTER_PATTERNS: {}",
                pattern, e
            );
            false
        }
    }
}

/// Fetch all new container packages since `cutoff` **and** their versions
/// created since `cutoff`.
pub async fn get_new_packages(
    org: &str,
    cutoff: DateTime<Utc>,
) -> Result<Vec<PackageWithVersions>, Box<dyn Error>> {
    let github_token_parameter_store_key = env::var("OCI_PULL_GITHUB_TOKEN_PARAMETER_STORE_KEY")
        .map_err(|_| "OCI_PULL_GITHUB_TOKEN_PARAMETER_STORE_KEY environment variable not set")?;
    let token = get_securestring_aws(&github_token_parameter_store_key).await?;
    let client = reqwest::Client::new();

    // 1) Fetch new packages
    let mut page = 1;
    let mut recent: Vec<PackageWithVersions> = Vec::new();

    loop {
        let url = format!(
            "https://api.github.com/orgs/{org}/packages?package_type=container&per_page=100&page={page}",
            org = org,
            page = page
        );
        let resp = client
            .get(&url)
            .header(header::ACCEPT, "application/vnd.github+json")
            .header(header::AUTHORIZATION, format!("Bearer {}", token))
            .header("X-GitHub-Api-Version", "2022-11-28")
            .header(header::USER_AGENT, "infraweave-oci-poller")
            .send()
            .await?
            .error_for_status()?;

        let page_pkgs: Vec<Package> = resp.json().await?;
        if page_pkgs.is_empty() {
            break;
        }

        let mut found_new = false;
        for pkg in page_pkgs.into_iter() {
            // Filter for infraweave packages by name
            if !is_infraweave_package(&pkg.name) {
                println!("Skipping non-infraweave package: {}", pkg.name);
                continue;
            }

            if pkg.updated_at > cutoff {
                found_new = true;
                // 2) For each new package, fetch its most recent versions
                let versions =
                    get_package_versions(&client, &token, org, &pkg.name, cutoff).await?;
                recent.push(PackageWithVersions {
                    package: pkg,
                    recent_versions: versions,
                });
            } else {
                println!(
                    "Skipping infraweave package {} which was last updated at {}",
                    pkg.name, pkg.updated_at
                );
            }
        }
        if !found_new {
            break;
        }
        page += 1;
    }

    Ok(recent)
}

/// Helper that pages through /versions and returns only those with created_at > cutoff.
async fn get_package_versions(
    client: &reqwest::Client,
    token: &str,
    org: &str,
    pkg_name: &str,
    cutoff: DateTime<Utc>,
) -> Result<Vec<PackageVersion>, Box<dyn Error>> {
    let mut page = 1;
    let mut recent = Vec::new();

    loop {
        let url = format!(
            "https://api.github.com/orgs/{org}/packages/container/{pkg}/versions?per_page=100&page={page}",
            org = org,
            pkg = pkg_name,
            page = page
        );
        let resp = client
            .get(&url)
            .header(header::ACCEPT, "application/vnd.github+json")
            .header(header::AUTHORIZATION, format!("Bearer {}", token))
            .header("X-GitHub-Api-Version", "2022-11-28")
            .header(header::USER_AGENT, "infraweave-oci-poller")
            .send()
            .await?
            .error_for_status()?;

        let page_vers: Vec<PackageVersion> = resp.json().await?;
        if page_vers.is_empty() {
            break;
        }

        let mut any = false;
        for v in page_vers {
            if v.created_at > cutoff {
                any = true;
                recent.push(v);
            }
        }
        if !any {
            break;
        }
        page += 1;
    }

    Ok(recent)
}

/// Convert PackageWithVersions from get_new_packages into GitHubPackageWebhook events
pub fn convert_packages_to_webhook_events(
    packages_with_versions: Vec<PackageWithVersions>,
    org: &str,
    action: &str, // "published" or "updated"
) -> Vec<GitHubPackageWebhook> {
    let mut events = Vec::new();

    for package_with_versions in packages_with_versions {
        let package = package_with_versions.package;

        // Create a synthetic repository structure
        let repository = GitHubRepository {
            full_name: format!("{}/{}", org, package.name),
            name: package.name.clone(),
            html_url: package.html_url.clone(),
            owner: GitHubOwner {
                login: org.to_string(),
            },
        };

        // Generate an event for each recent version
        for version in package_with_versions.recent_versions {
            let github_package = GitHubPackage {
                name: package.name.clone(),
                package_type: package.package_type.clone(),
                owner: GitHubOwner {
                    login: org.to_string(),
                },
                package_version: GitHubPackageVersion {
                    version: version.name.clone(),
                    package_url: Some(version.html_url.clone()),
                    container_metadata: extract_container_metadata_from_version(&version),
                },
                registry: Some(GitHubRegistry {
                    url: "ghcr.io".to_string(),
                }),
            };

            let webhook_event = GitHubPackageWebhook {
                action: action.to_string(),
                registry_package: Some(github_package.clone()),
                package: Some(github_package),
                repository: repository.clone(),
                installation: None, // Would need to be filled in if available
            };

            events.push(webhook_event);
        }
    }

    events
}

/// Extract container metadata from PackageVersion if available
fn extract_container_metadata_from_version(
    version: &PackageVersion,
) -> Option<GitHubContainerMetadata> {
    // Try to extract tags from metadata if it's structured container metadata
    if let Some(container_data) = version.metadata.get("container") {
        if let Some(tags_array) = container_data.get("tags") {
            if let Some(tags) = tags_array.as_array() {
                let tag_strings: Vec<String> = tags
                    .iter()
                    .filter_map(|tag| tag.as_str().map(|s| s.to_string()))
                    .collect();

                if !tag_strings.is_empty() {
                    return Some(GitHubContainerMetadata {
                        tags: Some(tag_strings.clone()),
                        tag: tag_strings.first().map(|tag_name| GitHubTag {
                            name: tag_name.clone(),
                        }),
                    });
                }
            }
        }
    }

    // Fallback: use the version name as a tag
    Some(GitHubContainerMetadata {
        tags: Some(vec![version.name.clone()]),
        tag: Some(GitHubTag {
            name: version.name.clone(),
        }),
    })
}

/// Generate synthetic package publish events for packages updated since cutoff
pub async fn generate_package_events_from_api(
    org: &str,
    cutoff: DateTime<Utc>,
) -> Result<Vec<GitHubPackageWebhook>, Box<dyn Error>> {
    let packages = get_new_packages(org, cutoff).await?;

    // Convert to webhook events - mark as "published" since they're new
    let events = convert_packages_to_webhook_events(packages, org, "published");

    Ok(events)
}

/// Process synthetic package events as if they came from webhooks
pub async fn process_synthetic_package_events(
    org: &str,
    cutoff: DateTime<Utc>,
) -> Result<Vec<serde_json::Value>, Box<dyn Error>> {
    let webhook_events = generate_package_events_from_api(org, cutoff).await?;
    let mut results = Vec::new();

    for webhook_event in webhook_events {
        // Convert to the format expected by handle_package_publish_event
        let synthetic_event = serde_json::json!({
            "body": serde_json::to_string(&webhook_event)?,
            "headers": {}
        });

        println!(
            "Processing synthetic event for package: {} version: {}",
            webhook_event
                .registry_package
                .as_ref()
                .or(webhook_event.package.as_ref())
                .map(|p| p.name.clone())
                .unwrap_or_else(|| "unknown".to_string()),
            webhook_event
                .registry_package
                .as_ref()
                .or(webhook_event.package.as_ref())
                .map(|p| p.package_version.version.clone())
                .unwrap_or_else(|| "unknown".to_string())
        );

        // Process using the existing webhook handler
        match handle_package_publish_event(&synthetic_event).await {
            Ok(result) => results.push(result),
            Err(e) => {
                println!("Error processing synthetic event: {:?}", e);
                results.push(serde_json::json!({
                    "statusCode": 500,
                    "error": format!("Error processing event: {}", e)
                }));
            }
        }
    }

    Ok(results)
}

pub async fn handle_package_publish_event(event: &Value) -> Result<Value, anyhow::Error> {
    println!("handle_package_publish_event: {:?}", event);

    // Parse the webhook event directly using serde
    let body_str = event.get("body").and_then(|b| b.as_str()).unwrap_or("");
    let webhook: GitHubPackageWebhook = serde_json::from_str(body_str)?;

    // Work directly with the webhook data
    let (artifact_type, detected_tag) = detect_artifact_type_and_tag_from_webhook(&webhook);
    let package_info = webhook
        .registry_package
        .as_ref()
        .or(webhook.package.as_ref());

    let package_name = package_info
        .map(|p| p.name.clone())
        .unwrap_or_else(|| "unknown".to_string());
    let package_version = package_info
        .map(|p| p.package_version.version.clone())
        .unwrap_or_else(|| "unknown".to_string());
    let package_url = package_info
        .and_then(|p| p.package_version.package_url.clone())
        .unwrap_or_default();

    println!(
        "Package publish event - Action: {}, Package: {}, Version: {}, Repository: {}",
        webhook.action, package_name, package_version, webhook.repository.full_name
    );

    println!("🎯 Detected artifact type: {:?}", artifact_type);
    println!("🏷️ Detected tag: {}", detected_tag);
    println!("📦 Package URL from webhook: {}", package_url);

    match webhook.action.as_str() {
        "published" => {
            // Handle package published event
            // Since the module OCI-artifact is always published first, we cannot run verifications on this trigger, this instead
            // happens at runtime when the module is actually used.
            println!(
                "Package {} version {} was published in repository {}",
                package_name, package_version, webhook.repository.full_name
            );

            let package_type = package_info
                .map(|p| p.package_type.clone())
                .unwrap_or_else(|| "container".to_string());
            let owner = package_info
                .map(|p| p.owner.login.clone())
                .unwrap_or_else(|| webhook.repository.owner.login.clone());

            println!("Package type: {}", package_type);
            println!("Owner: {}", owner);

            let github_token_parameter_store_key =
                std::env::var("OCI_PULL_GITHUB_TOKEN_PARAMETER_STORE_KEY").map_err(|_| {
                    anyhow::anyhow!(
                        "OCI_PULL_GITHUB_TOKEN_PARAMETER_STORE_KEY environment variable not set"
                    )
                })?;
            let token = get_securestring_aws(&github_token_parameter_store_key).await?;

            // Determine artifacts to process
            let artifacts_to_process = match artifact_type {
                ArtifactType::Attestation => {
                    vec![ArtifactType::Attestation, ArtifactType::Signature]
                } // Use the fact that attestation is always published after signature in InfraWeave actions
                _ => vec![artifact_type.clone()],
            };

            let handler = GenericCloudHandler::default().await;
            let all_regions = handler.get_all_regions().await?;

            let mut artifact_tasks = Vec::new();
            let mut main_package_digest: Option<String> = None;

            for artifact_type_it in &artifacts_to_process {
                // Construct proper OCI registry URL: ghcr.io/owner/package:tag
                let oci_tag = match artifact_type_it {
                    ArtifactType::Signature => detected_tag.replace(".att", ".sig"),
                    _ => detected_tag.clone(),
                };

                let oci_package_url = format!(
                    "ghcr.io/{}/{}:{}",
                    owner.to_lowercase(),
                    package_name.to_lowercase(),
                    oci_tag
                );

                println!(
                    "🔧 Processing artifact type: {:?} with tag: {}",
                    artifact_type_it, oci_tag
                );

                println!("🔧 OCI function parameters - Image: '{}'", oci_package_url);
                println!("🔑 Using GitHub App token for package");
                let (digest, tag) = env_utils::save_oci_artifacts_separate(
                    &oci_package_url,
                    &token,
                    &artifact_type_it,
                )
                .await?;
                println!("✓ OCI artifacts saved successfully:");

                // Store the digest for MainPackage artifact type
                if artifact_type_it == &ArtifactType::MainPackage {
                    main_package_digest = Some(digest.clone());
                }

                let artifact_path = format!("/tmp/{}.tar.gz", &tag);
                let oci_artifact_path: String = format!("oci-artifacts/{}.tar.gz", &tag); // Path in S3 bucket

                let upload_task = upload_oci_artifact_to_all_regions(
                    handler.clone(),
                    artifact_path,
                    oci_artifact_path,
                    all_regions.clone(),
                );
                artifact_tasks.push(upload_task);
            }

            println!("⏳ Awaiting all artifact upload tasks...");
            for upload_result in futures::future::join_all(artifact_tasks).await {
                upload_result?;
            }
            println!("✓ All artifact uploads completed successfully");

            let mut main_package_tasks = Vec::new();

            for artifact_type_it in &artifacts_to_process {
                if artifact_type_it == &ArtifactType::MainPackage {
                    let detected_tag_clone = detected_tag.clone();
                    let package_name_clone = package_name.clone();
                    let handler_clone = handler.clone();

                    let digest = main_package_digest.clone().ok_or_else(|| {
                        anyhow::anyhow!("MainPackage digest not found - this should not happen")
                    })?;

                    let main_package_task = process_main_package_artifact(
                        detected_tag_clone,
                        package_name_clone,
                        handler_clone,
                        digest,
                    );
                    main_package_tasks.push(main_package_task);
                }
            }

            // Await all main package processing tasks
            println!("⏳ Awaiting all main package processing tasks...");
            for main_package_result in futures::future::join_all(main_package_tasks).await {
                main_package_result?;
            }
            println!("✓ All main package processing completed successfully");

            Ok(serde_json::json!({
                "statusCode": 200,
                "body": format!("Package publish event processed successfully for {}", package_name),
            }))
        }
        "updated" => {
            // Handle package updated event
            println!(
                "Package {} was updated in repository {}",
                package_name, webhook.repository.full_name
            );

            Ok(serde_json::json!({
                "statusCode": 200,
                "body": format!("Package update event processed successfully for {}", package_name),
            }))
        }
        _ => {
            println!("Unsupported package action: {}", webhook.action);
            Ok(serde_json::json!({
                "statusCode": 200,
                "body": format!("Unsupported package action: {}", webhook.action),
            }))
        }
    }
}

async fn process_main_package_artifact(
    detected_tag: String,
    package_name: String,
    handler: GenericCloudHandler,
    digest: String,
) -> Result<(), anyhow::Error> {
    let oci_tag = detected_tag.clone();
    let tag = oci_tag;
    let artifact_path = format!("/tmp/{}.tar.gz", &tag);

    let module_zip = get_module_zip_from_oci_targz(&artifact_path).unwrap();
    let mut module: ModuleResp = get_module_manifest_from_oci_targz(&artifact_path).unwrap();

    // Restore original casing before going through normal publishing process
    if let Some(ref mut examples) = module.manifest.spec.examples {
        for example in examples.iter_mut() {
            example.variables = convert_module_example_variables_to_snake_case(&example.variables);
            println!("Converted example variables: {:?}", example.variables);
        }
    }

    match publish_module_from_zip(
        &handler,
        module.manifest,
        &module.track,
        &module_zip,
        Some(OciArtifactSet {
            oci_artifact_path: "oci-artifacts/".to_string(),
            tag_main: tag,
            tag_attestation: Some(format!("{}.att", &digest.replace(':', "-"))),
            tag_signature: Some(format!("{}.sig", &digest.replace(':', "-"))),
            digest: digest,
        }),
    )
    .await
    {
        Ok(_) => {
            println!("Module {} published successfully", package_name);
            Ok(())
        }
        Err(e) => {
            println!("Error publishing module {}: {:?}", package_name, e);
            Err(anyhow::anyhow!("Failed to publish module: {}", e))
        }
    }
}

async fn upload_oci_artifact_to_all_regions(
    handler: GenericCloudHandler,
    artifact_path: String,
    oci_artifact_path: String,
    all_regions: Vec<String>,
) -> Result<(), anyhow::Error> {
    let targz_base64 = env_utils::read_file_base64(Path::new(&artifact_path)).unwrap();

    let payload = serde_json::json!({
        "event": "upload_file_base64",
        "data":
        {
            "key": &oci_artifact_path,
            "bucket_name": "modules",
            "base64_content": &targz_base64
        }

    });

    println!(
        "Uploading module zip file to storage with key: {}",
        &oci_artifact_path
    );

    let concurrency_limit_env = std::env::var("CONCURRENCY_LIMIT")
        .unwrap_or_else(|_| "".to_string())
        .parse::<usize>()
        .unwrap_or(10);
    let concurrency_limit = std::cmp::min(all_regions.len(), concurrency_limit_env);

    let results: Vec<Result<(), anyhow::Error>> = stream::iter(all_regions.iter())
        .map(|region| {
            let handler = handler.clone();
            let region = region.clone();
            let payload_ref = &payload;
            async move {
                let region_handler = handler.copy_with_region(&region).await;
                match region_handler.run_function(payload_ref).await {
                    Ok(_) => {
                        info!("Successfully uploaded module zip file to oci-artifact storage in region: {}", region);
                        Ok(())
                    }
                    Err(error) => {
                        Err(anyhow::anyhow!("Failed to upload to region {}: {}", region, error))
                    }
                }
            }
        })
        .buffer_unordered(concurrency_limit)
        .collect()
        .await;

    for result in results {
        result?;
    }
    Ok(())
}

pub async fn handle_check_run_rerequested_event(
    body: &Value,
    headers: &Value,
) -> Result<Value, anyhow::Error> {
    let push_payload = get_check_run_rerequested_data(body, headers).await?;
    let wrapped_event = json!({
        "body": push_payload.to_string(), // Convert to string to mimic the original event
        "headers": headers.clone(),
    });

    handle_process_push_event(&wrapped_event).await
}

pub async fn get_check_run_rerequested_data(
    body: &Value,
    headers: &Value,
) -> Result<Value, anyhow::Error> {
    let head_sha = body["check_run"]["head_sha"]
        .as_str()
        .ok_or(anyhow::anyhow!("Missing head_sha"))?;
    let owner = body["repository"]["owner"]["login"]
        .as_str()
        .ok_or(anyhow::anyhow!("Missing repository owner"))?;
    let repo = body["repository"]["name"]
        .as_str()
        .ok_or(anyhow::anyhow!("Missing repository name"))?;
    let installation_id = body["installation"]["id"]
        .as_u64()
        .ok_or(anyhow::anyhow!("Missing installation id"))?;
    let app_id = headers
        .get("x-github-hook-installation-target-id")
        .and_then(|s| s.as_str())
        .unwrap_or("");

    // Get a token for this installation.
    let private_key =
        get_securestring_aws(&std::env::var("GITHUB_PRIVATE_KEY_PARAMETER_STORE_KEY")?).await?;
    let token = get_installation_token(installation_id, app_id, &private_key).unwrap();

    // Query commit details using the commit SHA.
    let url = format!(
        "{}/repos/{}/{}/commits/{}",
        GITHUB_API_URL, owner, repo, head_sha
    );
    let client = reqwest::blocking::Client::new();
    let mut commit = client
        .get(&url)
        .header("User-Agent", INFRAWEAVE_USER_AGENT)
        .header("Authorization", format!("token {}", token))
        .send()?
        .error_for_status()?
        .json::<serde_json::Value>()?;

    // Derive "added", "removed", and "modified" fields from the "files" array.
    if let Some(files) = commit.get("files").and_then(|v| v.as_array()) {
        let mut added = Vec::new();
        let mut removed = Vec::new();
        let mut modified = Vec::new();
        for file in files {
            if let (Some(status), Some(filename)) = (
                file.get("status").and_then(|v| v.as_str()),
                file.get("filename").and_then(|v| v.as_str()),
            ) {
                match status {
                    "added" => added.push(filename.to_string()),
                    "removed" => removed.push(filename.to_string()),
                    "modified" => modified.push(filename.to_string()),
                    _ => {}
                }
            }
        }
        commit["added"] = serde_json::json!(added);
        commit["removed"] = serde_json::json!(removed);
        commit["modified"] = serde_json::json!(modified);
    } else {
        // Fallback if the "files" array is missing.
        commit["added"] = serde_json::json!([]);
        commit["removed"] = serde_json::json!([]);
        commit["modified"] = serde_json::json!([]);
    }

    let before_sha = commit["parents"]
        .as_array()
        .and_then(|parents| parents.first())
        .and_then(|p| p["sha"].as_str())
        .unwrap_or("");

    let branch = body["check_run"]["head_branch"].as_str().unwrap_or("main");

    let author = serde_json::json!({
        "name":  &commit["commit"]["author"]["name"].as_str().unwrap_or(""),
        "email": &commit["commit"]["author"]["email"].as_str().unwrap_or(""),
    });
    let sender = body["sender"].clone();

    let push_payload = serde_json::json!({
        "ref": format!("refs/heads/{}", branch),
        "before": before_sha,
        "after": head_sha,
        "commits": [commit],
        "repository": body["repository"],
        "installation": body["installation"],
        "sender": sender,
        "head_commit": {
            "author": author,
        }
    });

    Ok(push_payload)
}

fn get_check_run_name(name: &str, path: &str, region: &str, namespace: &str) -> String {
    format!("{} ({}) - {} ({})", name, region, path, namespace)
}

async fn inform_missing_project_configuration(
    extra_data: &mut ExtraData,
    name: &str,
    private_key_pem: &str,
) {
    if let ExtraData::GitHub(ref mut github_check_run) = extra_data {
        github_check_run.check_run.name = name.to_string();
        github_check_run.check_run.status = "completed".to_string();
        github_check_run.check_run.conclusion = Some("failure".to_string());
        github_check_run.check_run.completed_at = Some(Utc::now().to_rfc3339());
        github_check_run.check_run.output = Some(CheckRunOutput {
            title: "This repository is not yet configured".into(),
            summary: "Failed to get project id and region for repository".into(),
            text: Some("## Error\nPlease check the configuration and make sure to assign it to a project_id and region".into()),
            annotations: None,
        });
        post_check_run_from_payload(github_check_run.to_owned(), private_key_pem)
            .await
            .unwrap();
    }
}

pub async fn post_check_run_from_payload(
    github_check_run: GitHubCheckRun,
    private_key_pem: &str,
) -> Result<Value, Box<dyn Error>> {
    let client = Client::new();
    let token = get_installation_token(
        github_check_run.installation.id,
        github_check_run.app_id.to_string().as_str(),
        private_key_pem,
    )
    .unwrap();

    let owner = github_check_run.repository.owner.login.as_str();
    let repo = github_check_run.repository.name.as_str();

    let body = serde_json::to_value(github_check_run.check_run)?;

    println!("GitHub check run: {:?}", body);

    // Post the check run to the GitHub Checks API.
    let check_run_url = format!("{}/repos/{}/{}/check-runs", GITHUB_API_URL, owner, repo);
    let check_run_response = client
        .post(&check_run_url)
        .header("Authorization", format!("token {}", token))
        .header("Accept", "application/vnd.github+json")
        .header("User-Agent", INFRAWEAVE_USER_AGENT)
        .json(&body)
        .send()?
        .error_for_status()?;
    let check_run_result: Value = check_run_response.json()?;

    Ok(check_run_result)
}

pub async fn poll_and_process_new_packages(
    org: &str,
    poll_interval_minutes: u64,
) -> Result<Vec<serde_json::Value>, Box<dyn Error>> {
    let cutoff = Utc::now() - chrono::Duration::minutes(poll_interval_minutes as i64);

    println!(
        "🔍 Polling for new packages in org '{}' since {} ({} minutes ago)",
        org,
        cutoff.format("%Y-%m-%d %H:%M:%S UTC"),
        poll_interval_minutes
    );

    let results = process_synthetic_package_events(org, cutoff).await?;

    println!("✅ Processed {} package events", results.len());

    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    static SECRET: &str = "my-github-webhook-secret"; // As in GitHub App settings
    static REQUEST: &str = r#"
{
    "headers": {
        "x-hub-signature-256": "sha256=d797845b50ccfea741edebe2bfd841735e7aa265dc6466d1afc0a59616a07d33"
    },
    "version": "2.0",
    "routeKey": "POST /webhook",
    "rawPath": "/webhook",
    "body": "{\"ref\":\"refs/heads/main\",\"before\":\"placeholder_before_commit\",\"after\":\"placeholder_after_commit\",\"repository\":{\"id\":123456789,\"node_id\":\"R_placeholder_node\",\"name\":\"example-repo\",\"full_name\":\"ExampleUser/example-repo\",\"private\":true,\"owner\":{\"name\":\"ExampleUser\",\"email\":\"example@example.com\",\"login\":\"ExampleUser\",\"id\":987654321,\"node_id\":\"U_placeholder\",\"avatar_url\":\"https://avatars.githubusercontent.com/u/987654321?v=4\",\"gravatar_id\":\"\",\"url\":\"https://api.github.com/users/ExampleUser\",\"html_url\":\"https://github.com/ExampleUser\",\"followers_url\":\"https://api.github.com/users/ExampleUser/followers\",\"following_url\":\"https://api.github.com/users/ExampleUser/following{/other_user}\",\"gists_url\":\"https://api.github.com/users/ExampleUser/gists{/gist_id}\",\"starred_url\":\"https://api.github.com/users/ExampleUser/starred{/owner}{/repo}\",\"subscriptions_url\":\"https://api.github.com/users/ExampleUser/subscriptions\",\"organizations_url\":\"https://api.github.com/users/ExampleUser/orgs\",\"repos_url\":\"https://api.github.com/users/ExampleUser/repos\",\"events_url\":\"https://api.github.com/users/ExampleUser/events{/privacy}\",\"received_events_url\":\"https://api.github.com/users/ExampleUser/received_events\",\"type\":\"User\",\"user_view_type\":\"public\",\"site_admin\":false},\"html_url\":\"https://github.com/ExampleUser/example-repo\",\"description\":null,\"fork\":false,\"url\":\"https://github.com/ExampleUser/example-repo\",\"forks_url\":\"https://api.github.com/repos/ExampleUser/example-repo/forks\",\"keys_url\":\"https://api.github.com/repos/ExampleUser/example-repo/keys{/key_id}\",\"collaborators_url\":\"https://api.github.com/repos/ExampleUser/example-repo/collaborators{/collaborator}\",\"teams_url\":\"https://api.github.com/repos/ExampleUser/example-repo/teams\",\"hooks_url\":\"https://api.github.com/repos/ExampleUser/example-repo/hooks\",\"issue_events_url\":\"https://api.github.com/repos/ExampleUser/example-repo/issues/events{/number}\",\"events_url\":\"https://api.github.com/repos/ExampleUser/example-repo/events\",\"assignees_url\":\"https://api.github.com/repos/ExampleUser/example-repo/assignees{/user}\",\"branches_url\":\"https://api.github.com/repos/ExampleUser/example-repo/branches{/branch}\",\"tags_url\":\"https://api.github.com/repos/ExampleUser/example-repo/tags\",\"blobs_url\":\"https://api.github.com/repos/ExampleUser/example-repo/git/blobs{/sha}\",\"git_tags_url\":\"https://api.github.com/repos/ExampleUser/example-repo/git/tags{/sha}\",\"git_refs_url\":\"https://api.github.com/repos/ExampleUser/example-repo/git/refs{/sha}\",\"trees_url\":\"https://api.github.com/repos/ExampleUser/example-repo/git/trees{/sha}\",\"statuses_url\":\"https://api.github.com/repos/ExampleUser/example-repo/statuses/{sha}\",\"languages_url\":\"https://api.github.com/repos/ExampleUser/example-repo/languages\",\"stargazers_url\":\"https://api.github.com/repos/ExampleUser/example-repo/stargazers\",\"contributors_url\":\"https://api.github.com/repos/ExampleUser/example-repo/contributors\",\"subscribers_url\":\"https://api.github.com/repos/ExampleUser/example-repo/subscribers\",\"subscription_url\":\"https://api.github.com/repos/ExampleUser/example-repo/subscription\",\"commits_url\":\"https://api.github.com/repos/ExampleUser/example-repo/commits{/sha}\",\"git_commits_url\":\"https://api.github.com/repos/ExampleUser/example-repo/git/commits{/sha}\",\"comments_url\":\"https://api.github.com/repos/ExampleUser/example-repo/comments{/number}\",\"issue_comment_url\":\"https://api.github.com/repos/ExampleUser/example-repo/issues/comments{/number}\",\"contents_url\":\"https://api.github.com/repos/ExampleUser/example-repo/contents/{+path}\",\"compare_url\":\"https://api.github.com/repos/ExampleUser/example-repo/compare/{base}...{head}\",\"merges_url\":\"https://api.github.com/repos/ExampleUser/example-repo/merges\",\"archive_url\":\"https://api.github.com/repos/ExampleUser/example-repo/{archive_format}{/ref}\",\"downloads_url\":\"https://api.github.com/repos/ExampleUser/example-repo/downloads\",\"issues_url\":\"https://api.github.com/repos/ExampleUser/example-repo/issues{/number}\",\"pulls_url\":\"https://api.github.com/repos/ExampleUser/example-repo/pulls{/number}\",\"milestones_url\":\"https://api.github.com/repos/ExampleUser/example-repo/milestones{/number}\",\"notifications_url\":\"https://api.github.com/repos/ExampleUser/example-repo/notifications{?since,all,participating}\",\"labels_url\":\"https://api.github.com/repos/ExampleUser/example-repo/labels{/name}\",\"releases_url\":\"https://api.github.com/repos/ExampleUser/example-repo/releases{/id}\",\"deployments_url\":\"https://api.github.com/repos/ExampleUser/example-repo/deployments\",\"created_at\":1600000000,\"updated_at\":\"2025-02-25T20:55:48Z\",\"pushed_at\":1600000500,\"git_url\":\"git://github.com/ExampleUser/example-repo.git\",\"ssh_url\":\"git@github.com:ExampleUser/example-repo.git\",\"clone_url\":\"https://github.com/ExampleUser/example-repo.git\",\"svn_url\":\"https://github.com/ExampleUser/example-repo\",\"homepage\":null,\"size\":1234,\"stargazers_count\":10,\"watchers_count\":10,\"language\":\"Rust\",\"has_issues\":true,\"has_projects\":true,\"has_downloads\":true,\"has_wiki\":false,\"has_pages\":false,\"has_discussions\":false,\"forks_count\":2,\"mirror_url\":null,\"archived\":false,\"disabled\":false,\"open_issues_count\":0,\"license\":null,\"allow_forking\":true,\"is_template\":false,\"web_commit_signoff_required\":false,\"topics\":[\"rust\",\"webhook\"],\"visibility\":\"private\",\"forks\":2,\"open_issues\":1,\"watchers\":15,\"default_branch\":\"main\",\"stargazers\":10,\"master_branch\":\"main\"},\"pusher\":{\"name\":\"ExampleUser\",\"email\":\"example@example.com\"},\"sender\":{\"login\":\"ExampleUser\",\"id\":987654321,\"node_id\":\"U_placeholder\",\"avatar_url\":\"https://avatars.githubusercontent.com/u/987654321?v=4\",\"gravatar_id\":\"\",\"url\":\"https://api.github.com/users/ExampleUser\",\"html_url\":\"https://github.com/ExampleUser\",\"followers_url\":\"https://api.github.com/users/ExampleUser/followers\",\"following_url\":\"https://api.github.com/users/ExampleUser/following{/other_user}\",\"gists_url\":\"https://api.github.com/users/ExampleUser/gists{/gist_id}\",\"starred_url\":\"https://api.github.com/users/ExampleUser/starred{/owner}{/repo}\",\"subscriptions_url\":\"https://api.github.com/users/ExampleUser/subscriptions\",\"organizations_url\":\"https://api.github.com/users/ExampleUser/orgs\",\"repos_url\":\"https://api.github.com/users/ExampleUser/repos\",\"events_url\":\"https://api.github.com/users/ExampleUser/events{/privacy}\",\"received_events_url\":\"https://api.github.com/users/ExampleUser/received_events\",\"type\":\"User\",\"user_view_type\":\"public\",\"site_admin\":false},\"installation\":{\"id\":11111111,\"node_id\":\"I_placeholder\"},\"created\":false,\"deleted\":false,\"forced\":false,\"base_ref\":null,\"compare\":\"https://github.com/ExampleUser/example-repo/compare/placeholder_before_commit...placeholder_after_commit\",\"commits\":[{\"id\":\"17a6dafbf2d4c318f16102f8840c5f3c4f9e367c\",\"tree_id\":\"placeholder_tree_id\",\"distinct\":true,\"message\":\"Update README\",\"timestamp\":\"2025-02-25T21:58:49+01:00\",\"url\":\"https://github.com/ExampleUser/example-repo/commit/placeholder_commit_id\",\"author\":{\"name\":\"ExampleUser\",\"email\":\"example@example.com\",\"username\":\"ExampleUser\"},\"committer\":{\"name\":\"GitHub\",\"email\":\"noreply@example.com\",\"username\":\"web-flow\"},\"added\":[],\"removed\":[],\"modified\":[\"README.md\"]}],\"head_commit\":{\"id\":\"17a6dafbf2d4c318f16102f8840c5f3c4f9e367c\",\"tree_id\":\"placeholder_tree_id\",\"distinct\":true,\"message\":\"Update README\",\"timestamp\":\"2025-02-25T21:58:49+01:00\",\"url\":\"https://github.com/ExampleUser/example-repo/commit/placeholder_commit_id\",\"author\":{\"name\":\"ExampleUser\",\"email\":\"example@example.com\",\"username\":\"ExampleUser\"},\"committer\":{\"name\":\"GitHub\",\"email\":\"noreply@example.com\",\"username\":\"web-flow\"},\"added\":[],\"removed\":[],\"modified\":[\"README.md\"]}}",
    "isBase64Encoded": false
}
"#; // Realistic request but some values are removed

    fn _compute_signature(body: &[u8], secret: &str) -> String {
        let mut mac =
            HmacSha256::new_from_slice(secret.as_bytes()).expect("HMAC can take key of any size");
        mac.update(body);
        let result = mac.finalize();
        let expected_bytes = result.into_bytes();
        format!("sha256={}", hex::encode(expected_bytes))
    }

    #[test]
    fn test_github_request_signature_verification() {
        let request: Value = serde_json::from_str(REQUEST).unwrap();
        let body_str = request["body"].as_str().unwrap();

        println!(
            "Computed signature: {}",
            _compute_signature(body_str.as_bytes(), SECRET)
        );

        let signature = request["headers"]["x-hub-signature-256"].as_str().unwrap();
        println!("{}", body_str);
        assert_eq!(
            verify_signature(body_str.as_bytes(), signature, SECRET),
            true
        );
    }
}
