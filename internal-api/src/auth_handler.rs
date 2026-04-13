use anyhow::{anyhow, Result};
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::OnceLock;

/// Cached OIDC configuration (lazily discovered)
static OIDC_CONFIG: OnceLock<OidcConfig> = OnceLock::new();

/// OIDC Discovery document (subset of fields we need)
#[derive(Debug, Deserialize)]
struct OidcDiscovery {
    authorization_endpoint: String,
    token_endpoint: String,
}

/// Resolved OIDC configuration for the token bridge
#[derive(Debug, Clone)]
struct OidcConfig {
    authorization_endpoint: String,
    token_endpoint: String,
    client_id: String,
    client_secret: Option<String>,
    scopes: String,
    allowed_redirect_uris: Vec<String>,
}

/// Initialize and cache the OIDC configuration.
///
/// Configuration is resolved from environment variables:
///
/// # Required
/// * `OIDC_CLIENT_ID` - The OAuth2 client ID registered with your identity provider
///
/// # Endpoint discovery (one of the following)
/// * `OIDC_ISSUER_URL` - The OIDC issuer URL (e.g., `https://accounts.google.com`,
///   `https://cognito-idp.us-west-2.amazonaws.com/us-west-2_xxx`,
///   `https://login.microsoftonline.com/{tenant}/v2.0`).
///   The authorization and token endpoints are auto-discovered via
///   `{issuer}/.well-known/openid-configuration`.
/// * `OIDC_AUTHORIZATION_ENDPOINT` + `OIDC_TOKEN_ENDPOINT` - Explicit endpoint URLs
///   (skips OIDC discovery). Both must be set together.
///
/// # Optional
/// * `OIDC_CLIENT_SECRET` - Client secret (required by some providers for confidential clients)
/// * `OIDC_SCOPES` - Space-separated scopes (default: `openid email profile`)
/// * `OIDC_ALLOWED_REDIRECT_URIS` - Comma-separated allowlist of redirect URIs
///   (default: `http://localhost:8080/callback`)
async fn get_oidc_config() -> Result<&'static OidcConfig> {
    if let Some(config) = OIDC_CONFIG.get() {
        return Ok(config);
    }

    let client_id = std::env::var("OIDC_CLIENT_ID")
        .map_err(|_| anyhow!("OIDC_CLIENT_ID environment variable not set"))?;

    let client_secret = std::env::var("OIDC_CLIENT_SECRET").ok();

    let scopes =
        std::env::var("OIDC_SCOPES").unwrap_or_else(|_| "openid email profile".to_string());

    let allowed_redirect_uris: Vec<String> = std::env::var("OIDC_ALLOWED_REDIRECT_URIS")
        .unwrap_or_else(|_| "http://localhost:8080/callback".to_string())
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    // Try explicit endpoints first, then OIDC discovery
    let (authorization_endpoint, token_endpoint) = match (
        std::env::var("OIDC_AUTHORIZATION_ENDPOINT").ok(),
        std::env::var("OIDC_TOKEN_ENDPOINT").ok(),
    ) {
        (Some(auth), Some(token)) => {
            log::info!("Using explicit OIDC endpoints");
            (auth, token)
        }
        _ => {
            let issuer_url = std::env::var("OIDC_ISSUER_URL").map_err(|_| {
                anyhow!(
                    "OIDC_ISSUER_URL environment variable not set. \
                     Set OIDC_ISSUER_URL for auto-discovery, or set both \
                     OIDC_AUTHORIZATION_ENDPOINT and OIDC_TOKEN_ENDPOINT."
                )
            })?;
            discover_oidc_endpoints(&issuer_url).await?
        }
    };

    let config = OidcConfig {
        authorization_endpoint,
        token_endpoint,
        client_id,
        client_secret,
        scopes,
        allowed_redirect_uris,
    };

    log::info!(
        "OIDC config initialized: auth_endpoint={}, token_endpoint={}",
        config.authorization_endpoint,
        config.token_endpoint,
    );

    Ok(OIDC_CONFIG.get_or_init(|| config))
}

/// Discover OIDC authorization and token endpoints from the issuer's
/// `.well-known/openid-configuration` document.
async fn discover_oidc_endpoints(issuer_url: &str) -> Result<(String, String)> {
    let discovery_url = format!(
        "{}/.well-known/openid-configuration",
        issuer_url.trim_end_matches('/')
    );

    log::info!("Discovering OIDC endpoints from {}", discovery_url);

    let client = reqwest::Client::new();
    let response = client.get(&discovery_url).send().await.map_err(|e| {
        anyhow!(
            "Failed to fetch OIDC discovery document from {}: {}",
            discovery_url,
            e
        )
    })?;

    if !response.status().is_success() {
        return Err(anyhow!(
            "OIDC discovery endpoint {} returned status {}",
            discovery_url,
            response.status()
        ));
    }

    let discovery: OidcDiscovery = response
        .json()
        .await
        .map_err(|e| anyhow!("Failed to parse OIDC discovery document: {}", e))?;

    log::info!(
        "Discovered OIDC endpoints: authorization={}, token={}",
        discovery.authorization_endpoint,
        discovery.token_endpoint,
    );

    Ok((discovery.authorization_endpoint, discovery.token_endpoint))
}

/// Generate an OIDC sign-in URL for the configured identity provider.
///
/// This builds a standard OAuth2 authorization code flow URL that works with
/// any OIDC-compliant provider (Cognito, Azure AD, Okta, Auth0, Google, etc.).
///
/// # Arguments
/// * `redirect_uri` - Where the IdP should redirect after authentication
///   (default: `http://localhost:8080/callback`)
///
/// # Returns
/// JSON with `sign_in_url` that the client should open in a browser
pub async fn generate_sign_in_url(redirect_uri: Option<String>) -> Result<Value> {
    let config = get_oidc_config().await?;

    let redirect_uri = redirect_uri.unwrap_or_else(|| "http://localhost:8080/callback".to_string());

    // Validate redirect URI against allowlist (if explicitly configured)
    if std::env::var("OIDC_ALLOWED_REDIRECT_URIS").is_ok()
        && !config
            .allowed_redirect_uris
            .iter()
            .any(|u| u == &redirect_uri)
    {
        return Err(anyhow!(
            "Redirect URI '{}' is not in the allowed list. \
             Allowed: {}",
            redirect_uri,
            config.allowed_redirect_uris.join(", ")
        ));
    }

    let encoded_redirect_uri = urlencoding::encode(&redirect_uri);
    let encoded_scopes = config.scopes.replace(' ', "+");

    let url = format!(
        "{}?client_id={}&response_type=code&scope={}&redirect_uri={}",
        config.authorization_endpoint, config.client_id, encoded_scopes, encoded_redirect_uri,
    );

    Ok(json!({
        "success": true,
        "sign_in_url": url,
        "message": "Please sign in through the provided URL to obtain tokens.",
        "instructions": "Visit the sign_in_url to authenticate via your identity provider."
    }))
}

/// Exchange an OAuth2 authorization code for tokens.
///
/// This uses the standard OIDC token endpoint and works with any compliant
/// identity provider (Cognito, Azure AD, Okta, Auth0, Google, etc.).
///
/// # Arguments
/// * `code` - The authorization code received from the IdP redirect
/// * `redirect_uri` - Must match the redirect_uri used in the authorize request
///
/// # Returns
/// JSON with `access_token`, `id_token`, `refresh_token`, etc.
pub async fn exchange_code_for_tokens(code: &str, redirect_uri: Option<String>) -> Result<Value> {
    let config = get_oidc_config().await?;

    let redirect_uri = redirect_uri.unwrap_or_else(|| "http://localhost:8080/callback".to_string());

    log::info!("Exchanging authorization code for tokens");

    let mut params = vec![
        ("grant_type".to_string(), "authorization_code".to_string()),
        ("client_id".to_string(), config.client_id.clone()),
        ("code".to_string(), code.to_string()),
        ("redirect_uri".to_string(), redirect_uri),
    ];

    if let Some(ref secret) = config.client_secret {
        params.push(("client_secret".to_string(), secret.clone()));
    }

    let client = reqwest::Client::new();
    let response = client
        .post(&config.token_endpoint)
        .form(&params)
        .send()
        .await
        .map_err(|e| anyhow!("Failed to call token endpoint: {}", e))?;

    let status = response.status();
    let body_text = response
        .text()
        .await
        .unwrap_or_else(|_| "Failed to read response body".to_string());

    if !status.is_success() {
        log::error!(
            "Token exchange failed with status {}: {}",
            status,
            body_text
        );
        return Err(anyhow!(
            "Token exchange failed (status {}): {}",
            status,
            body_text
        ));
    }

    let token_response: Value = serde_json::from_str(&body_text)
        .map_err(|e| anyhow!("Failed to parse token response: {}", e))?;

    log::info!("Successfully exchanged code for tokens");

    Ok(json!({
        "success": true,
        "access_token": token_response.get("access_token"),
        "id_token": token_response.get("id_token"),
        "refresh_token": token_response.get("refresh_token"),
        "token_type": token_response.get("token_type").and_then(|v| v.as_str()).unwrap_or("Bearer"),
        "expires_in": token_response.get("expires_in"),
    }))
}

/// Extract username from IAM ARN (AWS-specific utility)
#[cfg(feature = "aws")]
pub fn extract_username_from_arn(arn: &str) -> String {
    if arn.starts_with("arn:aws:iam::") {
        // IAM user: arn:aws:iam::123456789012:user/username
        if let Some(user_part) = arn.strip_prefix("arn:aws:iam::") {
            if let Some(username) = user_part.split('/').last() {
                return username.to_string();
            }
        }
    } else if arn.starts_with("arn:aws:sts::") {
        // Assumed role: arn:aws:sts::123456789012:assumed-role/role-name/session-name
        if let Some(parts) = arn.strip_prefix("arn:aws:sts::") {
            let segments: Vec<&str> = parts.split('/').collect();
            if segments.len() >= 3 {
                return segments.last().unwrap_or(&"unknown").to_string();
            }
        }
    }

    arn.split('/').last().unwrap_or("unknown").to_string()
}

/// Verify and extract user identity from IAM request context
///
/// This extracts the IAM user ARN from the API Gateway request context
/// when using IAM authorization.
#[cfg(feature = "aws")]
pub fn extract_iam_identity(request_context: &Value) -> Result<String> {
    // For IAM authorization, the identity is in requestContext.authorizer.iam
    if let Some(authorizer) = request_context.get("authorizer") {
        if let Some(iam) = authorizer.get("iam") {
            // Try userArn first (full ARN)
            if let Some(user_arn) = iam.get("userArn").and_then(|v| v.as_str()) {
                return Ok(user_arn.to_string());
            }

            // Try userId as fallback
            if let Some(user_id) = iam.get("userId").and_then(|v| v.as_str()) {
                return Ok(user_id.to_string());
            }
        }
    }

    // Fallback: Try legacy identity object (for older API Gateway versions)
    if let Some(identity) = request_context.get("identity") {
        if let Some(user_arn) = identity.get("userArn").and_then(|v| v.as_str()) {
            return Ok(user_arn.to_string());
        }

        if let Some(caller_arn) = identity.get("caller").and_then(|v| v.as_str()) {
            return Ok(caller_arn.to_string());
        }

        if let Some(account_id) = identity.get("accountId").and_then(|v| v.as_str()) {
            if let Some(user) = identity.get("user").and_then(|v| v.as_str()) {
                return Ok(format!("arn:aws:iam::{}:user/{}", account_id, user));
            }
        }
    }

    Err(anyhow!(
        "Could not extract IAM identity from request context"
    ))
}

/// Return the JWT claim key used for allowed projects.
///
/// Configurable via `AUTH_ALLOWED_PROJECTS_CLAIM` env var.
/// Defaults to `custom:allowed_projects` for backward compatibility with Cognito.
pub fn allowed_projects_claim_key() -> String {
    std::env::var("AUTH_ALLOWED_PROJECTS_CLAIM")
        .unwrap_or_else(|_| "custom:allowed_projects".to_string())
}

/// Return the JWT claim key used for publish permissions.
///
/// Configurable via `AUTH_PUBLISH_PERMISSIONS_CLAIM` env var.
/// Defaults to `custom:publish_permissions` for backward compatibility with Cognito.
pub fn publish_permissions_claim_key() -> String {
    std::env::var("AUTH_PUBLISH_PERMISSIONS_CLAIM")
        .unwrap_or_else(|_| "custom:publish_permissions".to_string())
}

/// Return the ordered list of JWT claim keys to try when resolving user identity.
///
/// Configurable via `AUTH_USERNAME_CLAIMS` env var (comma-separated).
/// Defaults to `sub,cognito:username,email` for backward compatibility.
pub fn username_claim_keys() -> Vec<String> {
    std::env::var("AUTH_USERNAME_CLAIMS")
        .unwrap_or_else(|_| "sub,cognito:username,email".to_string())
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[cfg(feature = "aws")]
    #[test]
    fn test_extract_username_from_iam_user_arn() {
        let arn = "arn:aws:iam::123456789012:user/alice";
        assert_eq!(extract_username_from_arn(arn), "alice");
    }

    #[cfg(feature = "aws")]
    #[test]
    fn test_extract_username_from_assumed_role_arn() {
        let arn = "arn:aws:sts::123456789012:assumed-role/MyRole/session-name";
        assert_eq!(extract_username_from_arn(arn), "session-name");
    }

    #[cfg(feature = "aws")]
    #[test]
    fn test_extract_username_from_plain_string() {
        let arn = "plain-user";
        assert_eq!(extract_username_from_arn(arn), "plain-user");
    }

    #[cfg(feature = "aws")]
    #[test]
    fn test_extract_username_with_nested_path() {
        let arn = "arn:aws:iam::123456789012:user/department/team/alice";
        assert_eq!(extract_username_from_arn(arn), "alice");
    }

    #[test]
    fn test_allowed_projects_claim_key_default() {
        let _lock = ENV_LOCK.lock().unwrap();
        // When env var is not set, should return Cognito default
        std::env::remove_var("AUTH_ALLOWED_PROJECTS_CLAIM");
        assert_eq!(allowed_projects_claim_key(), "custom:allowed_projects");
    }

    #[test]
    fn test_allowed_projects_claim_key_custom() {
        let _lock = ENV_LOCK.lock().unwrap();
        std::env::set_var("AUTH_ALLOWED_PROJECTS_CLAIM", "projects");
        assert_eq!(allowed_projects_claim_key(), "projects");
        std::env::remove_var("AUTH_ALLOWED_PROJECTS_CLAIM");
    }

    #[test]
    fn test_publish_permissions_claim_key_default() {
        let _lock = ENV_LOCK.lock().unwrap();
        std::env::remove_var("AUTH_PUBLISH_PERMISSIONS_CLAIM");
        assert_eq!(
            publish_permissions_claim_key(),
            "custom:publish_permissions"
        );
    }

    #[test]
    fn test_username_claim_keys_default() {
        let _lock = ENV_LOCK.lock().unwrap();
        std::env::remove_var("AUTH_USERNAME_CLAIMS");
        assert_eq!(
            username_claim_keys(),
            vec!["sub", "cognito:username", "email"]
        );
    }

    #[test]
    fn test_username_claim_keys_custom() {
        let _lock = ENV_LOCK.lock().unwrap();
        std::env::set_var("AUTH_USERNAME_CLAIMS", "sub,oid,email,upn");
        assert_eq!(username_claim_keys(), vec!["sub", "oid", "email", "upn"]);
        std::env::remove_var("AUTH_USERNAME_CLAIMS");
    }
}
