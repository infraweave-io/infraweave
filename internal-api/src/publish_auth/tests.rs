use super::*;
use serde_json::json;
use std::sync::{Mutex, MutexGuard};

static ENV_LOCK: Mutex<()> = Mutex::new(());

const GITHUB_OIDC_REGO: &str = include_str!("github_oidc.rego");
const JWT_USER_RAW_REGO: &str = include_str!("jwt_user_raw.rego");
const AWS_IAM_RAW_REGO: &str = include_str!("aws_iam_raw.rego");

const GITHUB_OIDC_ISS: &str = "https://token.actions.githubusercontent.com";
const COGNITO_ISS: &str = "https://cognito-idp.us-west-2.amazonaws.com/us-west-2_example";
const GITLAB_ISS: &str = "https://gitlab.com";
const OTHER_ISS: &str = "https://example.com";

/// Serialize tests on the shared env + policy cache and start each from a
/// clean slate. Hold the returned guard for the lifetime of the test.
fn test_setup() -> MutexGuard<'static, ()> {
    let guard = ENV_LOCK.lock().unwrap();
    reset_env();
    guard
}

fn reset_env() {
    for var in [
        "AUTH_PUBLISH_REGO_POLICY_PARAMETER",
        "AUTH_PUBLISH_REGO_POLICY_CACHE_TTL_SECONDS",
    ] {
        std::env::remove_var(var);
    }
    *policy_cache().lock().unwrap() = None;
}

fn publish_allowed(
    claims: &Value,
    resource_type: &str,
    resource_name: &str,
    track: Option<&str>,
) -> bool {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(check_publish(claims, resource_type, resource_name, track))
}

fn install_policy(policy: String) {
    std::env::set_var(
        "AUTH_PUBLISH_REGO_POLICY_PARAMETER",
        "/infraweave/test/publish-auth-rego",
    );
    cache_policy("/infraweave/test/publish-auth-rego".to_string(), policy).unwrap();
}

fn replace_set(policy: &str, name: &str, values: &[&str]) -> String {
    let set = format!(
        "{{{}}}",
        values
            .iter()
            .map(|v| format!("\"{}\"", v))
            .collect::<Vec<_>>()
            .join(", ")
    );
    policy.replace(
        &format!("{} := set()", name),
        &format!("{} := {}", name, set),
    )
}

fn use_github_oidc_policy() {
    install_policy(GITHUB_OIDC_REGO.to_string());
}

fn use_aws_iam_raw_policy_with_admin_arns(arns: &[&str]) {
    install_policy(replace_set(AWS_IAM_RAW_REGO, "admin_aws_role_arns", arns));
}

fn use_aws_iam_raw_policy_with_admin_ids(ids: &[&str]) {
    install_policy(replace_set(AWS_IAM_RAW_REGO, "admin_aws_role_ids", ids));
}

fn use_jwt_user_raw_policy_with(name: &str, values: &[&str]) {
    install_policy(replace_set(JWT_USER_RAW_REGO, name, values));
}

#[test]
fn check_publish_requires_policy_parameter_env_var() {
    let _guard = test_setup();
    let claims = json!({
        "iss": GITHUB_OIDC_ISS,
        "repository": "infraweave-io/tf-module-s3bucket",
        "repository_owner": "infraweave-io",
        "ref": "refs/heads/main"
    });

    assert!(!publish_allowed(
        &claims,
        "module",
        "s3bucket",
        Some("stable")
    ));

    use_github_oidc_policy();
    assert!(publish_allowed(
        &claims,
        "module",
        "s3bucket",
        Some("stable")
    ));
}

#[test]
fn detect_routes_github_issuer_to_github_oidc() {
    let claims = json!({ "iss": GITHUB_OIDC_ISS });
    assert_eq!(
        IdentityProvider::detect(&claims),
        IdentityProvider::GitHubOidc
    );
}

#[test]
fn detect_falls_back_to_raw_for_other_issuers() {
    let claims = json!({ "iss": GITLAB_ISS });
    assert_eq!(IdentityProvider::detect(&claims), IdentityProvider::Raw);

    let claims = json!({});
    assert_eq!(IdentityProvider::detect(&claims), IdentityProvider::Raw);
}

#[test]
fn rego_policy_allows_matching_publish() {
    let _guard = test_setup();
    use_github_oidc_policy();

    let claims = json!({
        "iss": GITHUB_OIDC_ISS,
        "repository": "infraweave-io/tf-module-s3bucket",
        "repository_owner": "infraweave-io",
        "ref": "refs/heads/main"
    });

    assert!(publish_allowed(
        &claims,
        "module",
        "s3bucket",
        Some("stable")
    ));
    assert!(!publish_allowed(&claims, "module", "eks", Some("stable")));
}

#[test]
fn rego_policy_denies_stable_publish_from_non_main_ref() {
    let _guard = test_setup();
    use_github_oidc_policy();

    let claims = json!({
        "iss": GITHUB_OIDC_ISS,
        "repository": "infraweave-io/tf-module-s3bucket",
        "repository_owner": "infraweave-io",
        "ref": "refs/heads/feature"
    });

    assert!(!publish_allowed(
        &claims,
        "module",
        "s3bucket",
        Some("stable")
    ));
}

#[test]
fn rego_policy_allows_dev_publish_from_non_main_ref() {
    let _guard = test_setup();
    use_github_oidc_policy();

    let claims = json!({
        "iss": GITHUB_OIDC_ISS,
        "repository": "infraweave-io/tf-module-s3bucket",
        "repository_owner": "infraweave-io",
        "ref": "refs/heads/feature"
    });

    assert!(publish_allowed(&claims, "module", "s3bucket", Some("dev")));
}

#[test]
fn rego_policy_allows_matching_policy_publish_without_track() {
    let _guard = test_setup();
    use_github_oidc_policy();

    let claims = json!({
        "iss": GITHUB_OIDC_ISS,
        "repository": "infraweave-io/tf-policy-guardrail",
        "repository_owner": "infraweave-io",
        "ref": "refs/heads/feature"
    });

    assert!(publish_allowed(&claims, "policy", "guardrail", None));
    assert!(!publish_allowed(&claims, "policy", "baseline", None));
}

#[test]
fn rego_policy_denies_cross_module_publish() {
    let _guard = test_setup();
    use_github_oidc_policy();

    let claims = json!({
        "iss": GITHUB_OIDC_ISS,
        "repository": "infraweave-io/tf-module-other",
        "repository_owner": "infraweave-io",
        "ref": "refs/heads/main"
    });

    assert!(!publish_allowed(
        &claims,
        "module",
        "s3bucket",
        Some("stable")
    ));
}

#[test]
fn rego_policy_denies_publish_from_foreign_tenant() {
    let _guard = test_setup();
    use_github_oidc_policy();

    let claims = json!({
        "iss": GITHUB_OIDC_ISS,
        "repository": "attacker-org/tf-module-s3bucket",
        "repository_owner": "attacker-org",
        "ref": "refs/heads/main"
    });

    assert!(!publish_allowed(
        &claims,
        "module",
        "s3bucket",
        Some("stable")
    ));
}

#[test]
fn rego_input_contains_identity_and_request() {
    let _guard = test_setup();
    let claims = json!({
        "iss": GITHUB_OIDC_ISS,
        "repository": "infraweave-io/modules",
        "repository_owner": "infraweave-io",
        "ref": "refs/heads/main",
        "job_workflow_ref": "infraweave-io/modules/.github/workflows/publish-module-s3bucket.yml@refs/heads/main",
        "environment": "publish-module-s3bucket"
    });
    let identity = IdentityProvider::GitHubOidc.extract(&claims).unwrap();

    let input = publish_rego_input(
        &IdentityProvider::GitHubOidc,
        &identity,
        "module",
        "s3bucket",
        Some("stable"),
    );

    assert_eq!(
        input.pointer("/identity/provider").and_then(Value::as_str),
        Some("github_oidc")
    );
    assert_eq!(
        input
            .pointer("/identity/workflow_ref")
            .and_then(Value::as_str),
        claims.get("job_workflow_ref").and_then(Value::as_str)
    );
    assert_eq!(
        input
            .pointer("/identity/workflow_name")
            .and_then(Value::as_str),
        Some("publish-module-s3bucket")
    );
    assert_eq!(
        input
            .pointer("/identity/environment")
            .and_then(Value::as_str),
        Some("publish-module-s3bucket")
    );
    assert_eq!(
        input
            .pointer("/identity/repository")
            .and_then(Value::as_str),
        Some("infraweave-io/modules")
    );
    assert_eq!(
        input
            .pointer("/request/resource_name")
            .and_then(Value::as_str),
        Some("s3bucket")
    );
}

#[test]
fn rego_input_supports_raw_claims() {
    let _guard = test_setup();
    let claims = json!({
        "iss": GITLAB_ISS,
        "sub": "project_path:infraweave/tf-module-s3bucket:ref_type:branch:ref:main",
        "project_path": "infraweave/tf-module-s3bucket"
    });
    let identity = IdentityProvider::Raw.extract(&claims).unwrap();

    let input = publish_rego_input(
        &IdentityProvider::Raw,
        &identity,
        "module",
        "s3bucket",
        Some("stable"),
    );

    assert_eq!(
        input.pointer("/identity/provider").and_then(Value::as_str),
        Some("raw")
    );
    assert_eq!(
        input.pointer("/identity/issuer").and_then(Value::as_str),
        Some(GITLAB_ISS)
    );
    assert_eq!(
        input
            .pointer("/identity/claims/project_path")
            .and_then(Value::as_str),
        Some("infraweave/tf-module-s3bucket")
    );
}

#[test]
fn aws_iam_raw_configured_role_arn_can_publish() {
    let _guard = test_setup();
    use_aws_iam_raw_policy_with_admin_arns(&["arn:aws:iam::123456789012:role/AdminRole"]);
    let claims = json!({
        "iss": OTHER_ISS,
        "aws_iam_arn": "arn:aws:sts::123456789012:assumed-role/AdminRole/alice@example.com",
    });
    assert!(publish_allowed(
        &claims,
        "module",
        "s3bucket",
        Some("stable")
    ));
}

#[test]
fn aws_iam_raw_other_role_arn_cannot_publish() {
    let _guard = test_setup();
    use_aws_iam_raw_policy_with_admin_arns(&["arn:aws:iam::123456789012:role/AdminRole"]);
    let claims = json!({
        "iss": OTHER_ISS,
        "aws_iam_arn": "arn:aws:sts::123456789012:assumed-role/OtherRole/alice@example.com",
    });
    assert!(!publish_allowed(
        &claims,
        "module",
        "s3bucket",
        Some("stable")
    ));
}

#[test]
fn aws_iam_raw_configured_role_id_can_publish() {
    let _guard = test_setup();
    use_aws_iam_raw_policy_with_admin_ids(&["AROAEXAMPLE123456"]);
    let claims = json!({
        "iss": OTHER_ISS,
        "aws_iam_user_id": "AROAEXAMPLE123456:alice@example.com",
    });
    assert!(publish_allowed(
        &claims,
        "module",
        "s3bucket",
        Some("stable")
    ));
}

#[test]
fn aws_iam_raw_other_role_id_cannot_publish() {
    let _guard = test_setup();
    use_aws_iam_raw_policy_with_admin_ids(&["AROAEXAMPLE123456"]);
    let claims = json!({
        "iss": OTHER_ISS,
        "aws_iam_user_id": "AROADIFFERENT123:alice@example.com",
    });
    assert!(!publish_allowed(
        &claims,
        "module",
        "s3bucket",
        Some("stable")
    ));
}

#[test]
fn jwt_user_raw_configured_email_can_publish() {
    let _guard = test_setup();
    use_jwt_user_raw_policy_with("admin_jwt_emails", &["alice@example.com"]);
    let claims = json!({
        "iss": OTHER_ISS,
        "sub": "user-123",
        "email": "Alice@Example.com",
    });
    assert!(publish_allowed(
        &claims,
        "module",
        "s3bucket",
        Some("stable")
    ));
}

#[test]
fn jwt_user_raw_other_email_cannot_publish() {
    let _guard = test_setup();
    use_jwt_user_raw_policy_with("admin_jwt_emails", &["alice@example.com"]);
    let claims = json!({
        "iss": OTHER_ISS,
        "sub": "user-456",
        "email": "bob@example.com",
    });
    assert!(!publish_allowed(
        &claims,
        "module",
        "s3bucket",
        Some("stable")
    ));
}

#[test]
fn jwt_user_raw_identity_center_user_id_can_publish() {
    let _guard = test_setup();
    use_jwt_user_raw_policy_with("admin_jwt_emails", &["alice@example.com"]);
    let claims = json!({
        "iss": COGNITO_ISS,
        "sub": "user-123",
        "cognito:username": "IdentityCenter_alice@example.com",
        "identities": [{
            "providerName": "IdentityCenter",
            "userId": "Alice@Example.com",
        }],
    });
    assert!(publish_allowed(
        &claims,
        "module",
        "s3bucket",
        Some("stable")
    ));
}

#[test]
fn jwt_user_raw_cognito_username_can_publish() {
    let _guard = test_setup();
    use_jwt_user_raw_policy_with("admin_jwt_usernames", &["IdentityCenter_alice@example.com"]);
    let claims = json!({
        "iss": COGNITO_ISS,
        "sub": "user-123",
        "cognito:username": "IdentityCenter_alice@example.com",
    });
    assert!(publish_allowed(
        &claims,
        "module",
        "s3bucket",
        Some("stable")
    ));
}

#[test]
fn jwt_user_raw_subject_fallback_can_publish() {
    let _guard = test_setup();
    use_jwt_user_raw_policy_with("admin_jwt_subjects", &["user-123"]);
    let claims = json!({
        "iss": OTHER_ISS,
        "sub": "user-123",
    });
    assert!(publish_allowed(
        &claims,
        "module",
        "s3bucket",
        Some("stable")
    ));
}

#[test]
fn jwt_user_raw_other_subject_fallback_cannot_publish() {
    let _guard = test_setup();
    use_jwt_user_raw_policy_with("admin_jwt_subjects", &["user-123"]);
    let claims = json!({
        "iss": OTHER_ISS,
        "sub": "user-456",
    });
    assert!(!publish_allowed(
        &claims,
        "module",
        "s3bucket",
        Some("stable")
    ));
}
