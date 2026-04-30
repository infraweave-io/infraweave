use anyhow::{anyhow, Result};
use aws_config::sts::AssumeRoleProvider;
use aws_config::{BehaviorVersion, Region, SdkConfig};
use aws_credential_types::provider::SharedCredentialsProvider;
use std::sync::OnceLock;

static CENTRAL_CONFIG: OnceLock<SdkConfig> = OnceLock::new();

pub fn central_config() -> Option<&'static SdkConfig> {
    CENTRAL_CONFIG.get()
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

    let sts = aws_sdk_sts::Client::new(&bootstrap);
    let identity = sts.get_caller_identity().send().await.map_err(|e| {
        anyhow!(
            "Failed to verify central role assumption {}: {:?}",
            role_arn,
            e
        )
    })?;

    let provider = AssumeRoleProvider::builder(role_arn.clone())
        .session_name("infraweave-central-session")
        .tags([("WorkloadAccount", identity.account().unwrap_or_default())])
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
