use anyhow::{anyhow, Result};
use serde_json::{json, Value};

#[cfg(feature = "aws")]
use aws_sdk_cognitoidentityprovider::Client as CognitoClient;

/// Check if a Cognito user exists and return sign-in URL
///
/// **IMPORTANT**: For federated Cognito users (SAML/Identity Center), tokens CANNOT be
/// generated programmatically - users must authenticate through their identity provider.
///
/// This function:
/// 1. Extracts username from IAM ARN
/// 2. Checks if the user exists in Cognito (tries multiple prefixes for federated users)
/// 3. Returns the Cognito hosted UI sign-in URL for authentication
///
/// # Arguments
/// * `user_id` - The IAM user ARN or identity (extracted from request context)
/// * `redirect_uri` - Required redirect URI (must be in COGNITO_ALLOWED_REDIRECT_URIS)
///
/// # Environment Variables
/// * `COGNITO_USER_POOL_ID` - The Cognito User Pool ID
/// * `COGNITO_DOMAIN` - The Cognito hosted UI domain (e.g., "your-domain.auth.us-west-2.amazoncognito.com")
/// * `COGNITO_CLIENT_ID` - The Cognito App Client ID
/// * `COGNITO_ALLOWED_REDIRECT_URIS` - Comma-separated allowlist of redirect URIs
/// * `COGNITO_USERNAME_PREFIXES` - Optional: Comma-separated list of prefixes to try
///   Examples:
///   - ",IdentityCenter_" (default) - Try as-is, then with IdentityCenter_ prefix
///   - "IdentityCenter_" - Try only with IdentityCenter_ prefix
///   - ",IdentityCenter_,Okta_" - Try as-is, then with IdentityCenter_, then with Okta_
///
/// # Required IAM Permissions
/// * `cognito-idp:AdminGetUser`
///
/// # Returns
/// JSON response with user information and sign-in URL
#[cfg(feature = "aws")]
pub async fn generate_token_for_iam_user(
    user_id: &str,
    redirect_uri: Option<String>,
) -> Result<Value> {
    let user_pool_id = std::env::var("COGNITO_USER_POOL_ID")
        .map_err(|_| anyhow!("COGNITO_USER_POOL_ID environment variable not set"))?;

    // Get AWS config, potentially overriding region if inferred from User Pool ID
    let mut config_loader = aws_config::from_env();

    // If user_pool_id starts with a region (e.g., us-west-2_...), enforce that region
    if let Some((region_name, _)) = user_pool_id.split_once('_') {
        // We use the Region type re-exported by the SDK
        let region = aws_sdk_cognitoidentityprovider::config::Region::new(region_name.to_string());
        config_loader = config_loader.region(region);
        log::info!(
            "Configuring Cognito client for region '{}' (inferred from User Pool ID)",
            region_name
        );
    }

    let config = config_loader.load().await;
    let cognito_client = CognitoClient::new(&config);

    // Extract username from IAM ARN
    let username = extract_username_from_arn(user_id);

    log::info!(
        "Looking up Cognito user for IAM identity: {} (username: {})",
        user_id,
        username
    );

    // Try to find user with various username formats (for federated users)
    let user_info = lookup_cognito_user(&cognito_client, &user_pool_id, &username).await?;

    let user_status = user_info.user_status();
    let actual_username = user_info.username();

    log::info!(
        "✓ Found Cognito user '{}' (Status: {:?})",
        actual_username,
        user_status
    );

    // Build the Cognito hosted UI sign-in URL (use actual username from Cognito)
    let sign_in_url = build_cognito_sign_in_url(actual_username, redirect_uri)?;

    // Return user information with sign-in URL
    Ok(json!({
        "success": true,
        "username": actual_username,
        "user_id": user_id,
        "user_status": format!("{:?}", user_status),
        "user_pool_id": user_pool_id,
        "sign_in_url": sign_in_url,
        "message": "User exists in Cognito. Please sign in through the provided URL to obtain tokens.",
        "instructions": "Visit the sign_in_url to authenticate via your identity provider and obtain Cognito tokens."
    }))
}

/// Lookup Cognito user with support for federated username prefixes
///
/// Tries username formats based on COGNITO_USERNAME_PREFIXES:
/// - If list contains empty string "", tries username as-is
/// - For each non-empty prefix, tries prefix + username
///
/// Example: COGNITO_USERNAME_PREFIXES=",IdentityCenter_,Okta_"
/// Will try: "username", "IdentityCenter_username", "Okta_username"
#[cfg(feature = "aws")]
async fn lookup_cognito_user(
    client: &CognitoClient,
    user_pool_id: &str,
    username: &str,
) -> Result<aws_sdk_cognitoidentityprovider::operation::admin_get_user::AdminGetUserOutput> {
    // Get prefixes to try (default to trying as-is then IdentityCenter_)
    let prefixes_str = std::env::var("COGNITO_USERNAME_PREFIXES")
        .unwrap_or_else(|_| ",IdentityCenter_".to_string());

    let mut tried_usernames = Vec::new();

    for prefix in prefixes_str.split(',').map(|s| s.trim()) {
        let candidate_username = format!("{}{}", prefix, username);

        tried_usernames.push(candidate_username.clone());
        log::info!("Trying Cognito username: {}", candidate_username);

        match client
            .admin_get_user()
            .user_pool_id(user_pool_id)
            .username(&candidate_username)
            .send()
            .await
        {
            Ok(info) => {
                log::info!("✓ Found Cognito user: {}", candidate_username);
                return Ok(info);
            }
            Err(e) => {
                log::warn!(
                    "✗ AdminGetUser failed for '{}': {:?}",
                    candidate_username,
                    e
                );
            }
        }
    }

    Err(anyhow!(
        "User '{}' not found in Cognito User Pool '{}'. Tried usernames: {}. Configure COGNITO_USERNAME_PREFIXES with correct prefix(es), use '' for no prefix.",
        username,
        user_pool_id,
        tried_usernames.join(", ")
    ))
}

/// Build the Cognito hosted UI sign-in URL
#[cfg(feature = "aws")]
fn build_cognito_sign_in_url(username: &str, redirect_uri: Option<String>) -> Result<String> {
    let domain = std::env::var("COGNITO_DOMAIN")
        .map_err(|_| anyhow!("COGNITO_DOMAIN environment variable not set (e.g., 'your-domain.auth.us-west-2.amazoncognito.com')"))?;

    let client_id = std::env::var("COGNITO_CLIENT_ID")
        .map_err(|_| anyhow!("COGNITO_CLIENT_ID environment variable not set"))?;

    // Default to CLI localhost callback (can override via request body)
    let redirect_uri = redirect_uri.unwrap_or_else(|| "http://localhost:8080/callback".to_string());

    // URL-encode the redirect URI
    let encoded_redirect_uri = urlencoding::encode(&redirect_uri);

    // Build the OAuth2 authorize URL
    // Using login_hint to pre-fill the username
    let url = format!(
        "https://{}/oauth2/authorize?client_id={}&response_type=code&scope=openid+email+profile&redirect_uri={}&login_hint={}",
        domain,
        client_id,
        encoded_redirect_uri,
        urlencoding::encode(username)
    );

    Ok(url)
}

/// Exchange authorization code for Cognito tokens
///
/// This function exchanges the authorization code (received after user signs in)
/// for actual Cognito access and ID tokens.
///
/// # Arguments
/// * `code` - The authorization code from Cognito hosted UI redirect
/// * `redirect_uri` - The redirect URI (must match what was used in authorize request)
///
/// # Environment Variables
/// * `COGNITO_DOMAIN` - The Cognito hosted UI domain
/// * `COGNITO_CLIENT_ID` - The Cognito App Client ID
///
/// # Returns
/// JSON response with access_token, id_token, refresh_token, and expires_in
#[cfg(feature = "aws")]
pub async fn exchange_code_for_tokens(code: &str, redirect_uri: Option<String>) -> Result<Value> {
    let domain = std::env::var("COGNITO_DOMAIN")
        .map_err(|_| anyhow!("COGNITO_DOMAIN environment variable not set"))?;

    let client_id = std::env::var("COGNITO_CLIENT_ID")
        .map_err(|_| anyhow!("COGNITO_CLIENT_ID environment variable not set"))?;

    // Default to CLI localhost callback (must match what was used in authorize request)
    let redirect_uri = redirect_uri.unwrap_or_else(|| "http://localhost:8080/callback".to_string());

    let token_url = format!("https://{}/oauth2/token", domain);

    log::info!("Exchanging authorization code for Cognito tokens");

    // Build form data for token exchange
    let params = [
        ("grant_type", "authorization_code"),
        ("client_id", &client_id),
        ("code", code),
        ("redirect_uri", &redirect_uri),
    ];

    // Make HTTP POST request to Cognito token endpoint
    let client = reqwest::Client::new();
    let response = client
        .post(&token_url)
        .form(&params)
        .send()
        .await
        .map_err(|e| anyhow!("Failed to call Cognito token endpoint: {}", e))?;

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
            "Cognito token exchange failed (status {}): {}",
            status,
            body_text
        ));
    }

    // Parse the response
    let token_response: Value = serde_json::from_str(&body_text)
        .map_err(|e| anyhow!("Failed to parse Cognito token response: {}", e))?;

    log::info!("✓ Successfully exchanged code for Cognito tokens");

    Ok(json!({
        "access_token": token_response.get("access_token"),
        "id_token": token_response.get("id_token"),
        "refresh_token": token_response.get("refresh_token"),
        "token_type": token_response.get("token_type").and_then(|v| v.as_str()).unwrap_or("Bearer"),
        "expires_in": token_response.get("expires_in"),
    }))
}

/// Extract username from IAM ARN
fn extract_username_from_arn(arn: &str) -> String {
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
                // Use session name if available, otherwise role name
                return segments.last().unwrap_or(&"unknown").to_string();
            }
        }
    }

    // Fallback: use the entire ARN or "unknown"
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
        // Try userArn first (for IAM users)
        if let Some(user_arn) = identity.get("userArn").and_then(|v| v.as_str()) {
            return Ok(user_arn.to_string());
        }

        // Try caller ARN (for assumed roles)
        if let Some(caller_arn) = identity.get("caller").and_then(|v| v.as_str()) {
            return Ok(caller_arn.to_string());
        }

        // Try accountId + user combination
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_username_from_iam_user_arn() {
        let arn = "arn:aws:iam::123456789012:user/alice";
        assert_eq!(extract_username_from_arn(arn), "alice");
    }

    #[test]
    fn test_extract_username_from_assumed_role_arn() {
        let arn = "arn:aws:sts::123456789012:assumed-role/MyRole/session-name";
        assert_eq!(extract_username_from_arn(arn), "session-name");
    }

    #[test]
    fn test_extract_username_from_plain_string() {
        let arn = "plain-user";
        assert_eq!(extract_username_from_arn(arn), "plain-user");
    }

    #[test]
    fn test_extract_username_with_nested_path() {
        let arn = "arn:aws:iam::123456789012:user/department/team/alice";
        assert_eq!(extract_username_from_arn(arn), "alice");
    }
}
