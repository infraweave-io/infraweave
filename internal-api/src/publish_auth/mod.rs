//! Provider-agnostic publish authorization.
//!
//! The model has two pieces, deliberately decoupled from any specific
//! identity provider (GitHub, GitLab, ...):
//!
//! 1. [`PublishIdentity`] - trusted identity facts extracted from JWT claims.
//! 2. [`IdentityProvider`] - knows how to fill a [`PublishIdentity`] from a
//!    provider's claim shape. First-class providers live in their own
//!    submodules (e.g. [`github_oidc`]); everything else falls back to [`raw`].
//! 3. Rego policy - receives `identity` and `request` input and returns the
//!    publish allow/deny decision.
//!
//! The active provider is auto-detected from the JWT `iss` claim. The rule is
//! enabled by setting `AUTH_PUBLISH_REGO_POLICY_PARAMETER` to the cloud
//! parameter name (AWS SSM Parameter Store, Azure App Configuration, ...)
//! holding the Rego policy; without it, publishes are denied. The actual read
//! is delegated to `env_common::interface::read_config_parameter` so that
//! internal-api stays provider-agnostic.

mod github_oidc;
mod raw;

use anyhow::{anyhow, Result};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex, OnceLock},
    time::{Duration, Instant},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublishIdentity {
    pub facts: serde_json::Map<String, Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdentityProvider {
    /// GitHub Actions OIDC. Exposes trusted GitHub facts; Rego decides which
    /// fact is authoritative for the requested publish operation.
    GitHubOidc,
    /// Generic raw fallback. Exposes trusted facts to Rego without
    /// provider-specific parsing.
    Raw,
}

/// Issuer URL for GitHub Actions OIDC tokens. The `iss` claim is verified
/// upstream during JWT validation, so it is safe to use for provider routing.
const GITHUB_ACTIONS_ISSUER: &str = "https://token.actions.githubusercontent.com";

impl IdentityProvider {
    /// Pick a provider from the JWT `iss` claim. GitHub Actions tokens route to
    /// the first-class extractor; everything else falls through to the raw
    /// extractor, where the Rego policy is expected to pin the relevant facts.
    pub fn detect(claims: &Value) -> Self {
        let issuer = claims.get("iss").and_then(|v| v.as_str()).unwrap_or("");
        if issuer == GITHUB_ACTIONS_ISSUER {
            Self::GitHubOidc
        } else {
            Self::Raw
        }
    }

    pub fn extract(&self, claims: &Value) -> Option<PublishIdentity> {
        match self {
            Self::GitHubOidc => github_oidc::extract(claims),
            Self::Raw => raw::extract(claims),
        }
    }

    fn as_str(&self) -> &'static str {
        match self {
            Self::GitHubOidc => "github_oidc",
            Self::Raw => "raw",
        }
    }
}

/// Top-level entry: auto-detect the provider from claims, then evaluate the
/// Rego policy. Returns `false` if the policy SSM parameter is unset, claims
/// don't yield an identity, or policy evaluation fails.
pub async fn check_publish(
    claims: &Value,
    resource_type: &str,
    resource_name: &str,
    track: Option<&str>,
) -> bool {
    if std::env::var("AUTH_PUBLISH_REGO_POLICY_PARAMETER").is_err() {
        return false;
    }
    let provider = IdentityProvider::detect(claims);
    let Some(identity) = provider.extract(claims) else {
        log::warn!(
            "Publish auth denied: provider={} did not yield an identity",
            provider.as_str()
        );
        return false;
    };

    match check_publish_with_rego(&provider, &identity, resource_type, resource_name, track).await {
        Ok(allowed) => {
            if !allowed {
                log_publish_denial(&provider, &identity, resource_type, resource_name, track);
            }
            allowed
        }
        Err(e) => {
            log::warn!("Failed to evaluate publish Rego policy: {}", e);
            false
        }
    }
}

fn log_publish_denial(
    provider: &IdentityProvider,
    identity: &PublishIdentity,
    resource_type: &str,
    resource_name: &str,
    track: Option<&str>,
) {
    let email = identity.facts.get("email").and_then(Value::as_str);
    let claims_email = identity
        .facts
        .get("claims")
        .and_then(|claims| claims.get("email"))
        .and_then(Value::as_str);
    let identity_center_user_id = identity
        .facts
        .get("claims")
        .and_then(|claims| claims.get("identities"))
        .and_then(Value::as_array)
        .and_then(|identities| {
            identities.iter().find_map(|identity| {
                let provider_name = identity.get("providerName").and_then(Value::as_str)?;
                if provider_name == "IdentityCenter" {
                    identity.get("userId").and_then(Value::as_str)
                } else {
                    None
                }
            })
        });
    let subject_present = identity.facts.get("subject").is_some();
    let parameter_name =
        std::env::var("AUTH_PUBLISH_REGO_POLICY_PARAMETER").unwrap_or_else(|_| "<unset>".into());

    log::warn!(
        "Publish auth denied by Rego: provider={}, resource={}/{}/{}, email_present={}, claims_email_present={}, identity_center_user_id_present={}, subject_present={}, policy_parameter={}",
        provider.as_str(),
        resource_type,
        resource_name,
        track.unwrap_or("-"),
        email.is_some(),
        claims_email.is_some(),
        identity_center_user_id.is_some(),
        subject_present,
        parameter_name
    );
}

async fn check_publish_with_rego(
    provider: &IdentityProvider,
    identity: &PublishIdentity,
    resource_type: &str,
    resource_name: &str,
    track: Option<&str>,
) -> Result<bool> {
    let mut engine = regorus::Engine::new();
    engine.add_policy(
        "publish_auth.rego".to_string(),
        publish_rego_policy().await?,
    )?;
    engine.set_input(json_to_rego_value(&publish_rego_input(
        provider,
        identity,
        resource_type,
        resource_name,
        track,
    ))?);

    let result = engine.eval_query("data.infraweave.publish.allow".to_string(), false)?;
    let Some(result) = result.result.first() else {
        return Ok(false);
    };
    let Some(expression) = result.expressions.first() else {
        return Ok(false);
    };

    match &expression.value {
        regorus::Value::Bool(allowed) => Ok(*allowed),
        other => Err(anyhow!(
            "publish policy returned non-boolean allow value: {:?}",
            other
        )),
    }
}

#[derive(Clone)]
struct CachedPolicy {
    parameter_name: String,
    policy: String,
    expires_at: Instant,
}

static POLICY_CACHE: OnceLock<Mutex<Option<CachedPolicy>>> = OnceLock::new();

async fn publish_rego_policy() -> Result<String> {
    let parameter_name = std::env::var("AUTH_PUBLISH_REGO_POLICY_PARAMETER")
        .map_err(|_| anyhow!("AUTH_PUBLISH_REGO_POLICY_PARAMETER must be set"))?;

    if let Some(policy) = cached_policy(&parameter_name)? {
        return Ok(policy);
    }

    let policy = env_common::interface::read_config_parameter(&parameter_name)
        .await
        .map_err(|e| {
            anyhow!(
                "failed to read publish Rego policy parameter '{}': {}",
                parameter_name,
                e
            )
        })?;
    if policy.trim().is_empty() {
        return Err(anyhow!(
            "publish Rego policy parameter '{}' is empty",
            parameter_name
        ));
    }
    cache_policy(parameter_name, policy.clone())?;
    Ok(policy)
}

fn policy_cache() -> &'static Mutex<Option<CachedPolicy>> {
    POLICY_CACHE.get_or_init(|| Mutex::new(None))
}

fn cached_policy(parameter_name: &str) -> Result<Option<String>> {
    let cache = policy_cache()
        .lock()
        .map_err(|_| anyhow!("publish policy cache mutex poisoned"))?;

    let Some(cached) = cache.as_ref() else {
        return Ok(None);
    };

    if cached.parameter_name == parameter_name && cached.expires_at > Instant::now() {
        return Ok(Some(cached.policy.clone()));
    }

    Ok(None)
}

fn cache_policy(parameter_name: String, policy: String) -> Result<()> {
    let mut cache = policy_cache()
        .lock()
        .map_err(|_| anyhow!("publish policy cache mutex poisoned"))?;
    *cache = Some(CachedPolicy {
        parameter_name,
        policy,
        expires_at: Instant::now() + policy_cache_ttl(),
    });
    Ok(())
}

fn policy_cache_ttl() -> Duration {
    let seconds = std::env::var("AUTH_PUBLISH_REGO_POLICY_CACHE_TTL_SECONDS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(300);
    Duration::from_secs(seconds)
}

fn publish_rego_input(
    provider: &IdentityProvider,
    identity: &PublishIdentity,
    resource_type: &str,
    resource_name: &str,
    track: Option<&str>,
) -> Value {
    let mut identity_facts = serde_json::Map::new();
    identity_facts.insert("provider".to_string(), serde_json::json!(provider.as_str()));
    identity_facts.extend(identity.facts.clone());

    let mut request = serde_json::Map::new();
    request.insert("action".to_string(), serde_json::json!("publish"));
    request.insert(
        "resource_type".to_string(),
        serde_json::json!(resource_type),
    );
    request.insert(
        "resource_name".to_string(),
        serde_json::json!(resource_name),
    );
    if let Some(track) = track {
        request.insert("track".to_string(), serde_json::json!(track));
    }

    serde_json::json!({
        "identity": identity_facts,
        "request": request,
    })
}

fn json_to_rego_value(json: &Value) -> Result<regorus::Value> {
    match json {
        Value::Null => Ok(regorus::Value::Null),
        Value::Bool(b) => Ok(regorus::Value::Bool(*b)),
        Value::Number(n) => {
            let Some(f) = n.as_f64() else {
                return Err(anyhow!("invalid JSON number for Rego input"));
            };
            Ok(regorus::Value::Number(f.into()))
        }
        Value::String(s) => Ok(regorus::Value::String(Arc::from(s.as_str()))),
        Value::Array(arr) => {
            let mut values = Vec::with_capacity(arr.len());
            for value in arr {
                values.push(json_to_rego_value(value)?);
            }
            Ok(regorus::Value::Array(Arc::new(values)))
        }
        Value::Object(obj) => {
            let mut values = BTreeMap::new();
            for (key, value) in obj {
                values.insert(
                    regorus::Value::String(Arc::from(key.as_str())),
                    json_to_rego_value(value)?,
                );
            }
            Ok(regorus::Value::Object(Arc::new(values)))
        }
    }
}

#[cfg(test)]
mod tests;
