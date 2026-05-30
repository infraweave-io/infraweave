//! GitHub Actions OIDC extractor.
//!
//! Maps GitHub OIDC claims onto trusted identity facts:
//! - `repository`
//! - `repository_name` (repository without owner)
//! - `repository_owner` (fallback: owner from `repository`)
//! - `ref` (or `:ref:` segment of `sub`)
//! - `workflow_ref` / `workflow_name` when `job_workflow_ref` is present
//! - `environment` when present

use serde_json::Value;

use super::PublishIdentity;

pub(super) fn extract(claims: &Value) -> Option<PublishIdentity> {
    let repository = claims.get("repository").and_then(|v| v.as_str())?;
    let (owner, name) = repository.split_once('/')?;
    if name.is_empty() {
        return None;
    }

    let context = claims
        .get("ref")
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .or_else(|| {
            claims
                .get("sub")
                .and_then(|v| v.as_str())
                .and_then(|sub| sub.split_once(":ref:").map(|(_, r)| r.to_string()))
        });

    let tenant = claims
        .get("repository_owner")
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .unwrap_or_else(|| owner.to_string());

    let mut facts = serde_json::Map::new();
    copy_claim(claims, &mut facts, "iss", "issuer");
    facts.insert(
        "repository".to_string(),
        Value::String(repository.to_string()),
    );
    facts.insert(
        "repository_name".to_string(),
        Value::String(name.to_string()),
    );
    facts.insert("repository_owner".to_string(), Value::String(tenant));
    if let Some(context) = context {
        facts.insert("ref".to_string(), Value::String(context));
    }
    if let Some(workflow_name) = workflow_name_from_claims(claims) {
        facts.insert("workflow_name".to_string(), Value::String(workflow_name));
    }
    copy_claim(claims, &mut facts, "job_workflow_ref", "workflow_ref");
    copy_claim(claims, &mut facts, "environment", "environment");

    Some(PublishIdentity { facts })
}

/// `job_workflow_ref` looks like
/// `org/repo/.github/workflows/publish-s3bucket.yml@refs/heads/main`.
/// We expose the filename stem as `workflow_name`.
fn workflow_name_from_claims(claims: &Value) -> Option<String> {
    let raw = claims.get("job_workflow_ref").and_then(|v| v.as_str())?;
    let path = raw.split('@').next()?;
    let file = path.rsplit('/').next()?;
    let stem = file
        .strip_suffix(".yml")
        .or_else(|| file.strip_suffix(".yaml"))
        .unwrap_or(file);
    if stem.is_empty() {
        None
    } else {
        Some(stem.to_string())
    }
}

fn copy_claim(
    claims: &Value,
    target: &mut serde_json::Map<String, Value>,
    claim_key: &str,
    target_key: &str,
) {
    if let Some(value) = claims.get(claim_key) {
        target.insert(target_key.to_string(), value.clone());
    }
}

#[cfg(test)]
mod tests {
    use super::super::IdentityProvider;
    use serde_json::json;
    use serde_json::Value;

    #[test]
    fn maps_repo_to_identity_facts() {
        let claims = json!({
            "repository": "infraweave-io/tf-module-s3bucket",
            "ref": "refs/heads/main",
            "repository_owner": "infraweave-io"
        });
        let id = IdentityProvider::GitHubOidc.extract(&claims).unwrap();
        assert_eq!(
            id.facts.get("repository_name").and_then(Value::as_str),
            Some("tf-module-s3bucket")
        );
        assert_eq!(
            id.facts.get("ref").and_then(Value::as_str),
            Some("refs/heads/main")
        );
        assert_eq!(
            id.facts.get("repository_owner").and_then(Value::as_str),
            Some("infraweave-io")
        );
    }

    #[test]
    fn falls_back_to_sub_for_ref() {
        let claims = json!({
            "repository": "infraweave-io/tf-module-s3bucket",
            "sub": "repo:infraweave-io/tf-module-s3bucket:ref:refs/heads/feature"
        });
        let id = IdentityProvider::GitHubOidc.extract(&claims).unwrap();
        assert_eq!(
            id.facts.get("ref").and_then(Value::as_str),
            Some("refs/heads/feature")
        );
    }

    #[test]
    fn falls_back_to_owner_from_repository_when_owner_claim_missing() {
        let claims = json!({
            "repository": "infraweave-io/tf-module-s3bucket",
            "ref": "refs/heads/main"
        });
        let id = IdentityProvider::GitHubOidc.extract(&claims).unwrap();
        assert_eq!(
            id.facts.get("repository_owner").and_then(Value::as_str),
            Some("infraweave-io")
        );
    }

    #[test]
    fn rejects_repository_without_slash() {
        let claims = json!({ "repository": "no-slash", "ref": "refs/heads/main" });
        assert!(IdentityProvider::GitHubOidc.extract(&claims).is_none());
    }

    #[test]
    fn exposes_workflow_and_environment_facts_when_present() {
        let claims = json!({
            "repository": "infraweave-io/modules",
            "ref": "refs/heads/main",
            "job_workflow_ref": "infraweave-io/modules/.github/workflows/publish-module-s3bucket.yaml@refs/heads/main",
            "environment": "publish-module-s3bucket"
        });
        let id = IdentityProvider::GitHubOidc.extract(&claims).unwrap();
        assert_eq!(
            id.facts.get("workflow_name").and_then(Value::as_str),
            Some("publish-module-s3bucket")
        );
        assert_eq!(
            id.facts.get("workflow_ref").and_then(Value::as_str),
            Some("infraweave-io/modules/.github/workflows/publish-module-s3bucket.yaml@refs/heads/main")
        );
        assert_eq!(
            id.facts.get("environment").and_then(Value::as_str),
            Some("publish-module-s3bucket")
        );
    }
}
