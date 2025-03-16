use base64::decode;
use chrono::Utc;
use env_common::interface::GenericCloudHandler;
use env_common::logic::{publish_notification, run_claim};
use env_defs::{
    CheckRun, CheckRunOutput, DeploymentManifest, ExtraData, GitHubCheckRun, Installation,
    JobDetails, NotificationData, Owner, Repository,
};
use futures::stream::{self, StreamExt};
use hmac::{Hmac, Mac};
use jsonwebtoken::{encode, EncodingKey, Header};
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::Sha256;
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
    let response = client
        .post(&url)
        .header("Authorization", format!("Bearer {}", jwt))
        .header("Accept", "application/vnd.github+json")
        .header("User-Agent", INFRAWEAVE_USER_AGENT)
        .send()?
        .error_for_status()?;

    let token_response: InstallationTokenResponse = response.json()?;
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

    let (project_id, _region, project_id_found) =
        match get_project_id_for_repository_path(repo_full_name).await {
            Ok((project_id, _region)) => (project_id, _region, true),
            Err(e) => {
                println!("Error getting project id: {:?}", e);
                (
                    "NOT_FOUND_FOR_REPO".to_string(),
                    "NOT_FOUND_FOR_REPO".to_string(),
                    false,
                )
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
                        github_check_run.job_details.manifest_yaml = canonical.clone();
                        if let Some(new_name) = yaml["metadata"]["name"].as_str() {
                            github_check_run.check_run.name =
                                get_check_run_name(new_name, &active.path, &github_check_run.job_details.region);
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
                                canonical
                            )),
                            annotations: None,
                        });
                    }
                    match serde_yaml::from_value::<DeploymentManifest>(yaml.clone()) {
                        Ok(deployment_claim) => {
                            let region = &deployment_claim.spec.region;
                            let handler = GenericCloudHandler::workload(project_id, region).await;
                            let flags = vec![];
                            match run_claim(
                                &handler,
                                &yaml,
                                &format!("git/{}", repo_full_name),
                                command,
                                flags,
                                extra_data.clone(),
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
                        github_check_run.job_details.manifest_yaml = canonical.clone();
                        if let Some(new_name) = yaml["metadata"]["name"].as_str() {
                            github_check_run.check_run.name =
                                get_check_run_name(new_name, &deleted.path, &github_check_run.job_details.region);
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
                                canonical
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
                            match run_claim(
                                &handler,
                                &yaml,
                                &format!("git/{}", repo_full_name),
                                command,
                                flags,
                                extra_data.clone(),
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

    let push_payload = serde_json::json!({
        "ref": format!("refs/heads/{}", branch),
        "before": before_sha,
        "after": head_sha,
        "commits": [commit],
        "repository": body["repository"],
        "installation": body["installation"],
    });

    Ok(push_payload)
}

fn get_check_run_name(name: &str, path: &str, region: &str) -> String {
    format!("{} ({}) - {}", name, region, path)
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
