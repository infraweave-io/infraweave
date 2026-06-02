use anyhow::{anyhow, Result};
use aws_config::sts::AssumeRoleProvider;
use aws_config::{BehaviorVersion, Region, SdkConfig};
use aws_credential_types::provider::SharedCredentialsProvider;
use std::collections::HashMap;
use std::future::Future;
use std::sync::LazyLock;
use std::sync::OnceLock;
use std::time::{Duration, SystemTime};
use tokio::sync::RwLock;

static CENTRAL_CONFIG: OnceLock<SdkConfig> = OnceLock::new();
static WORKLOAD_CONFIGS: LazyLock<RwLock<HashMap<String, CachedWorkloadConfig>>> =
    LazyLock::new(|| RwLock::new(HashMap::new()));
static PUBLISH_CONFIGS: LazyLock<RwLock<HashMap<PublishScope, CachedWorkloadConfig>>> =
    LazyLock::new(|| RwLock::new(HashMap::new()));

const WORKLOAD_ACCOUNT_TAG: &str = "WorkloadAccount";
const WORKLOAD_ACCESS_ROLE_ARN: &str = "INFRAWEAVE_WORKLOAD_ACCESS_ROLE_ARN";
const PUBLISH_ACCESS_ROLE_ARN: &str = "INFRAWEAVE_PUBLISH_ACCESS_ROLE_ARN";
const PUBLISH_TYPE_TAG: &str = "PublishType";
const PUBLISH_NAME_TAG: &str = "PublishName";
const PUBLISH_TRACK_TAG: &str = "PublishTrack";
const WORKLOAD_CONFIG_EXPIRY_SKEW: Duration = Duration::from_secs(300);
const ROLE_SESSION_NAME_MAX_LEN: usize = 64;

#[derive(Clone)]
struct CachedWorkloadConfig {
    config: SdkConfig,
    expires_at: SystemTime,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub(crate) struct PublishScope {
    resource_type: String,
    resource_name: String,
    track: Option<String>,
}

tokio::task_local! {
    static REQUEST_WORKLOAD_ACCOUNT: String;
}

tokio::task_local! {
    static REQUEST_PUBLISH_SCOPE: PublishScope;
}

pub fn central_config() -> Option<&'static SdkConfig> {
    CENTRAL_CONFIG.get()
}

pub async fn with_workload_account<F, T>(workload_account: String, future: F) -> T
where
    F: Future<Output = T>,
{
    REQUEST_WORKLOAD_ACCOUNT
        .scope(workload_account, future)
        .await
}

pub(crate) fn current_workload_account() -> Option<String> {
    REQUEST_WORKLOAD_ACCOUNT.try_with(Clone::clone).ok()
}

pub async fn with_publish_scope<F, T>(
    resource_type: String,
    resource_name: String,
    track: Option<String>,
    future: F,
) -> T
where
    F: Future<Output = T>,
{
    REQUEST_PUBLISH_SCOPE
        .scope(
            PublishScope {
                resource_type,
                resource_name,
                track,
            },
            future,
        )
        .await
}

pub(crate) fn current_publish_scope() -> Option<PublishScope> {
    REQUEST_PUBLISH_SCOPE.try_with(Clone::clone).ok()
}

pub fn ensure_workload_access_role_configured() -> Result<()> {
    workload_access_role_arn().map(|_| ())
}

pub fn ensure_publish_access_role_configured() -> Result<()> {
    publish_access_role_arn().map(|_| ())
}

pub async fn init_central_credentials(region: &str) -> Result<()> {
    if CENTRAL_CONFIG.get().is_some() {
        return Ok(());
    }
    let role_arn = match std::env::var("INFRAWEAVE_CENTRAL_ROLE_ARN") {
        Ok(v) if !v.is_empty() => v,
        _ => return Ok(()),
    };

    let bootstrap = aws_config::from_env()
        .region(Region::new(region.to_string()))
        .load()
        .await;

    let provider = AssumeRoleProvider::builder(role_arn.clone())
        .session_name("infraweave-central-session")
        .configure(&bootstrap)
        .build()
        .await;

    let assumed = bootstrap
        .into_builder()
        .credentials_provider(SharedCredentialsProvider::new(provider))
        .behavior_version(BehaviorVersion::latest())
        .build();

    let sts = aws_sdk_sts::Client::new(&assumed);
    let identity = sts.get_caller_identity().send().await.map_err(|e| {
        anyhow!(
            "Failed to verify central role assumption {}: {:?}",
            role_arn,
            e
        )
    })?;
    log::info!(
        "Assumed central role {} (arn={:?}, account={:?})",
        role_arn,
        identity.arn(),
        identity.account()
    );

    CENTRAL_CONFIG
        .set(assumed)
        .map_err(|_| anyhow!("central config already initialized"))?;
    Ok(())
}

pub(crate) async fn workload_config(
    workload_account: &str,
    region: Option<&str>,
) -> Result<SdkConfig> {
    let now = SystemTime::now();
    if let Some(cached) = WORKLOAD_CONFIGS.read().await.get(workload_account).cloned() {
        if cached.expires_at > now + WORKLOAD_CONFIG_EXPIRY_SKEW {
            return Ok(config_for_region(cached.config, region));
        }
    }

    let mut configs = WORKLOAD_CONFIGS.write().await;
    if let Some(cached) = configs.get(workload_account).cloned() {
        if cached.expires_at > now + WORKLOAD_CONFIG_EXPIRY_SKEW {
            return Ok(config_for_region(cached.config, region));
        }
    }

    let config_region = region.map(str::to_string).or_else(|| {
        std::env::var("AWS_REGION")
            .ok()
            .or_else(|| std::env::var("AWS_DEFAULT_REGION").ok())
    });
    let bootstrap = load_bootstrap_config(config_region.as_deref()).await;
    let sts = aws_sdk_sts::Client::new(&bootstrap);
    let role_arn = workload_access_role_arn()?;
    let session_name = role_session_name("infraweave-workload", &[workload_account]);
    let tag = aws_sdk_sts::types::Tag::builder()
        .key(WORKLOAD_ACCOUNT_TAG)
        .value(workload_account)
        .build()?;

    log::info!(
        "Assuming workload access role {} with {}={}",
        role_arn,
        WORKLOAD_ACCOUNT_TAG,
        workload_account
    );

    let assumed_role = sts
        .assume_role()
        .role_arn(&role_arn)
        .role_session_name(session_name)
        .tags(tag)
        .send()
        .await
        .map_err(|e| {
            anyhow!(
                "Failed to assume workload access role {}: {:?}",
                role_arn,
                e
            )
        })?;

    let credentials = assumed_role
        .credentials()
        .ok_or_else(|| anyhow!("No credentials returned from workload role assumption"))?;
    let expires_at = SystemTime::try_from(*credentials.expiration())
        .map_err(|e| anyhow!("Invalid workload role credential expiration: {}", e))?;

    let creds = aws_credential_types::Credentials::new(
        credentials.access_key_id(),
        credentials.secret_access_key(),
        Some(credentials.session_token().to_string()),
        Some(expires_at),
        "InfraweaveWorkloadAssumedRole",
    );

    let assumed = bootstrap
        .into_builder()
        .credentials_provider(SharedCredentialsProvider::new(creds))
        .behavior_version(BehaviorVersion::latest())
        .build();

    configs.insert(
        workload_account.to_string(),
        CachedWorkloadConfig {
            config: assumed.clone(),
            expires_at,
        },
    );

    Ok(config_for_region(assumed, region))
}

pub(crate) async fn publish_config(
    scope: &PublishScope,
    region: Option<&str>,
) -> Result<SdkConfig> {
    let now = SystemTime::now();
    if let Some(cached) = PUBLISH_CONFIGS.read().await.get(scope).cloned() {
        if cached.expires_at > now + WORKLOAD_CONFIG_EXPIRY_SKEW {
            return Ok(config_for_region(cached.config, region));
        }
    }

    let mut configs = PUBLISH_CONFIGS.write().await;
    if let Some(cached) = configs.get(scope).cloned() {
        if cached.expires_at > now + WORKLOAD_CONFIG_EXPIRY_SKEW {
            return Ok(config_for_region(cached.config, region));
        }
    }

    let config_region = region.map(str::to_string).or_else(|| {
        std::env::var("AWS_REGION")
            .ok()
            .or_else(|| std::env::var("AWS_DEFAULT_REGION").ok())
    });
    let bootstrap = load_bootstrap_config(config_region.as_deref()).await;
    let sts = aws_sdk_sts::Client::new(&bootstrap);
    let role_arn = publish_access_role_arn()?;
    let session_name = role_session_name(
        "infraweave-publish",
        &[&scope.resource_type, &scope.resource_name],
    );
    let publish_type_tag = aws_sdk_sts::types::Tag::builder()
        .key(PUBLISH_TYPE_TAG)
        .value(&scope.resource_type)
        .build()?;
    let publish_name_tag = aws_sdk_sts::types::Tag::builder()
        .key(PUBLISH_NAME_TAG)
        .value(&scope.resource_name)
        .build()?;
    let publish_track = scope.track.as_deref().unwrap_or("-");
    let publish_track_tag = aws_sdk_sts::types::Tag::builder()
        .key(PUBLISH_TRACK_TAG)
        .value(publish_track)
        .build()?;

    log::info!(
        "Assuming publish access role {} with {}={}, {}={}, and {}={}",
        role_arn,
        PUBLISH_TYPE_TAG,
        scope.resource_type,
        PUBLISH_NAME_TAG,
        scope.resource_name,
        PUBLISH_TRACK_TAG,
        publish_track
    );

    let assumed_role = sts
        .assume_role()
        .role_arn(&role_arn)
        .role_session_name(session_name)
        .tags(publish_type_tag)
        .tags(publish_name_tag)
        .tags(publish_track_tag)
        .send()
        .await
        .map_err(|e| {
            anyhow!(
                "Failed to assume publish access role {} for {}/{}: {:?}",
                role_arn,
                scope.resource_type,
                scope.resource_name,
                e
            )
        })?;

    let credentials = assumed_role
        .credentials()
        .ok_or_else(|| anyhow!("No credentials returned from publish role assumption"))?;
    let expires_at = SystemTime::try_from(*credentials.expiration())
        .map_err(|e| anyhow!("Invalid publish role credential expiration: {}", e))?;

    let creds = aws_credential_types::Credentials::new(
        credentials.access_key_id(),
        credentials.secret_access_key(),
        Some(credentials.session_token().to_string()),
        Some(expires_at),
        "InfraweavePublishAssumedRole",
    );

    let assumed = bootstrap
        .into_builder()
        .credentials_provider(SharedCredentialsProvider::new(creds))
        .behavior_version(BehaviorVersion::latest())
        .build();

    configs.insert(
        scope.clone(),
        CachedWorkloadConfig {
            config: assumed.clone(),
            expires_at,
        },
    );

    Ok(config_for_region(assumed, region))
}

async fn load_bootstrap_config(region: Option<&str>) -> SdkConfig {
    let mut loader = aws_config::from_env();
    if let Some(region) = region {
        loader = loader.region(Region::new(region.to_string()));
    }
    loader.load().await
}

fn config_for_region(config: SdkConfig, region: Option<&str>) -> SdkConfig {
    match region {
        Some(r) if config.region().map(|reg| reg.as_ref()) != Some(r) => config
            .into_builder()
            .region(Region::new(r.to_string()))
            .build(),
        _ => config,
    }
}

fn workload_access_role_arn() -> Result<String> {
    std::env::var(WORKLOAD_ACCESS_ROLE_ARN).map_err(|_| {
        anyhow!(
            "{} must be set for workload-scoped data access",
            WORKLOAD_ACCESS_ROLE_ARN
        )
    })
}

fn publish_access_role_arn() -> Result<String> {
    std::env::var(PUBLISH_ACCESS_ROLE_ARN).map_err(|_| {
        anyhow!(
            "{} must be set for publish-scoped catalog access",
            PUBLISH_ACCESS_ROLE_ARN
        )
    })
}

fn role_session_name(prefix: &str, parts: &[&str]) -> String {
    let mut name = sanitize_role_session_component(prefix);
    for part in parts {
        name.push('-');
        name.push_str(&sanitize_role_session_component(part));
    }

    if name.len() > ROLE_SESSION_NAME_MAX_LEN {
        name.truncate(ROLE_SESSION_NAME_MAX_LEN);
    }

    if name.len() < 2 {
        "iw".to_string()
    } else {
        name
    }
}

fn sanitize_role_session_component(value: &str) -> String {
    let sanitized: String = value
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '_' | '+' | '=' | ',' | '.' | '@' | '-') {
                c
            } else {
                '-'
            }
        })
        .collect();

    if sanitized.is_empty() {
        "unknown".to_string()
    } else {
        sanitized
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{LazyLock, Mutex};

    static ENV_LOCK: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

    #[test]
    fn ensure_workload_access_role_configured_requires_env_var() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::remove_var(WORKLOAD_ACCESS_ROLE_ARN);
        std::env::remove_var(PUBLISH_ACCESS_ROLE_ARN);

        let err = ensure_workload_access_role_configured().unwrap_err();

        assert!(err
            .to_string()
            .contains("INFRAWEAVE_WORKLOAD_ACCESS_ROLE_ARN must be set"));
    }

    #[test]
    fn ensure_workload_access_role_configured_accepts_env_var() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::set_var(
            WORKLOAD_ACCESS_ROLE_ARN,
            "arn:aws:iam::123456789012:role/infraweave-data-access",
        );

        assert!(ensure_workload_access_role_configured().is_ok());

        std::env::remove_var(WORKLOAD_ACCESS_ROLE_ARN);
    }

    #[test]
    fn ensure_publish_access_role_configured_requires_env_var() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::remove_var(WORKLOAD_ACCESS_ROLE_ARN);
        std::env::remove_var(PUBLISH_ACCESS_ROLE_ARN);

        let err = ensure_publish_access_role_configured().unwrap_err();

        assert!(err
            .to_string()
            .contains("INFRAWEAVE_PUBLISH_ACCESS_ROLE_ARN must be set"));
    }

    #[test]
    fn ensure_publish_access_role_configured_accepts_env_var() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::set_var(
            PUBLISH_ACCESS_ROLE_ARN,
            "arn:aws:iam::123456789012:role/infraweave-publish-access",
        );

        assert!(ensure_publish_access_role_configured().is_ok());

        std::env::remove_var(PUBLISH_ACCESS_ROLE_ARN);
    }

    #[tokio::test]
    async fn with_workload_account_sets_scope_only_inside_future() {
        assert_eq!(current_workload_account(), None);

        let account = with_workload_account("123456789012".to_string(), async {
            current_workload_account()
        })
        .await;

        assert_eq!(account.as_deref(), Some("123456789012"));
        assert_eq!(current_workload_account(), None);
    }

    #[tokio::test]
    async fn with_publish_scope_sets_scope_only_inside_future() {
        assert_eq!(current_publish_scope(), None);

        let scope = with_publish_scope(
            "module".to_string(),
            "s3bucket".to_string(),
            Some("dev".to_string()),
            async { current_publish_scope() },
        )
        .await;

        assert_eq!(
            scope,
            Some(PublishScope {
                resource_type: "module".to_string(),
                resource_name: "s3bucket".to_string(),
                track: Some("dev".to_string())
            })
        );
        assert_eq!(current_publish_scope(), None);
    }

    #[test]
    fn role_session_name_replaces_unsupported_characters() {
        let session_name = role_session_name("infraweave-publish", &["provider", "hashicorp/aws"]);

        assert_eq!(session_name, "infraweave-publish-provider-hashicorp-aws");
    }

    #[test]
    fn role_session_name_is_bounded_for_long_resource_names() {
        let session_name = role_session_name(
            "infraweave-publish",
            &[
                "provider",
                "a-very-long-provider-name-that-would-exceed-sts-limits",
            ],
        );

        assert!(session_name.len() <= ROLE_SESSION_NAME_MAX_LEN);
        assert!(session_name.starts_with("infraweave-publish-provider-"));
        assert!(session_name.chars().all(|c| {
            c.is_ascii_alphanumeric() || matches!(c, '_' | '+' | '=' | ',' | '.' | '@' | '-')
        }));
    }
}
