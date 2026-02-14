use axum::{
    extract::Request,
    http::StatusCode,
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use base64::{engine::general_purpose, Engine as _};
use jsonwebtoken::{decode, decode_header, Algorithm, DecodingKey, Validation};
use log::{debug, error, warn};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::OnceLock;

// Process-isolated token storage - only accessible within this process
static INTERNAL_TOKEN: OnceLock<String> = OnceLock::new();

/// Set the internal token for MCP authentication (call once at startup)
#[allow(dead_code)]
pub fn set_internal_token(token: String) {
    INTERNAL_TOKEN
        .set(token)
        .expect("Internal token already set");
}

/// Get the internal token if set
pub fn get_internal_token() -> Option<&'static str> {
    INTERNAL_TOKEN.get().map(|s| s.as_str())
}

/// JWT Claims structure with generic support for any claim
#[derive(Debug, Deserialize, Serialize)]
pub struct Claims {
    // Standard JWT claims
    pub sub: Option<String>, // Subject (user ID) - standard JWT claim
    pub iss: Option<String>, // Issuer
    pub aud: Option<String>, // Audience
    pub exp: Option<usize>,  // Expiration time
    pub iat: Option<usize>,  // Issued at time

    // Generic claims support - for dynamic claim extraction
    #[serde(flatten)]
    pub custom: std::collections::HashMap<String, serde_json::Value>,
}

/// JWKS (JSON Web Key Set) key structure
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct JwksKey {
    kty: String, // Key type
    #[serde(rename = "use")]
    key_use: Option<String>,
    kid: String,              // Key ID
    x5c: Option<Vec<String>>, // X.509 certificate chain
    n: Option<String>,        // RSA modulus
    e: Option<String>,        // RSA exponent
    k: Option<String>,        // Symmetric key (for HMAC)
}

#[derive(Debug, Deserialize)]
struct Jwks {
    keys: Vec<JwksKey>,
}

/// Project access context extracted from the request
#[derive(Debug, Clone)]
#[allow(dead_code)] // Used by handlers via request extensions
pub struct ProjectAccessContext {
    pub project_id: String,
    pub user_id: String,
}

/// Validate authentication configuration at startup
pub fn validate_auth_config() -> Vec<String> {
    let mut warnings = Vec::new();

    // Check JWT verification settings
    let disable_jwt_auth = std::env::var("DISABLE_JWT_AUTH_INSECURE")
        .unwrap_or_default()
        .to_lowercase()
        == "true";
    if disable_jwt_auth {
        warnings.push(
            "JWT authentication COMPLETELY DISABLED - INSECURE! Only use in development!"
                .to_string(),
        );
    } else {
        let has_jwks = std::env::var("JWKS_URL").is_ok();
        let has_issuer = std::env::var("JWT_ISSUER").is_ok();
        let has_static_key = std::env::var("JWT_SIGNING_KEY").is_ok();

        if !has_jwks && !has_issuer && !has_static_key {
            warnings.push("JWT verification enabled but no verification method configured. Set one of: JWT_SIGNING_KEY (for HMAC), JWKS_URL (for RSA), or JWT_ISSUER (auto-derives JWKS URL)".to_string());
        } else if has_static_key {
            warnings.push("Using static JWT signing key (HMAC-SHA256)".to_string());
        } else if has_jwks {
            warnings.push("Using JWKS endpoint for JWT verification".to_string());
        } else if has_issuer {
            warnings.push("Using JWT issuer to auto-derive JWKS endpoint".to_string());
        }
    }

    // Check audience configuration
    let audience = std::env::var("JWT_AUDIENCE").unwrap_or_else(|_| "infraweave-api".to_string());
    if audience == "infraweave-api" && std::env::var("JWT_AUDIENCE").is_err() {
        warnings.push(
            "Using default JWT audience 'infraweave-api' - set JWT_AUDIENCE to customize"
                .to_string(),
        );
    } else {
        warnings.push(format!("JWT audience validation enabled for: {}", audience));
    }

    warnings
}

/// Extract and validate JWT token from Authorization header
async fn extract_and_validate_jwt(auth_header: &str) -> Result<Claims, String> {
    // Extract bearer token
    if !auth_header.starts_with("Bearer ") {
        return Err("Invalid authorization header format".to_string());
    }

    let token = &auth_header[7..];

    // Check if JWT verification is enabled (for production)
    let verify_jwt = std::env::var("JWT_SIGNING_KEY").is_ok()
        || std::env::var("JWKS_URL").is_ok()
        || std::env::var("JWT_ISSUER").is_ok();

    if verify_jwt {
        log::info!("JWT verification enabled - verifying token signature");
        // Production mode: Verify JWT signature
        match verify_jwt_signature(token).await {
            Ok(claims) => Ok(claims),
            Err(e) => {
                error!("JWT verification failed: {}", e);
                Err(format!("JWT verification failed: {}", e))
            }
        }
    } else {
        // Development mode: Decode without verification but still validate audience
        warn!("JWT signature verification is disabled - only use in development!");
        let mut validation = Validation::new(Algorithm::RS256);
        validation.insecure_disable_signature_validation();
        validation.validate_exp = false;
        validation.validate_nbf = false; // Disable not-before validation

        // Configure audience - required even in development mode
        let expected_aud =
            std::env::var("JWT_AUDIENCE").unwrap_or_else(|_| "infraweave-api".to_string());
        validation.set_audience(std::slice::from_ref(&expected_aud));
        debug!(
            "JWT audience validation enabled for: {} (dev mode)",
            expected_aud
        );

        match decode::<Claims>(token, &DecodingKey::from_secret(b"dummy"), &validation) {
            Ok(token_data) => Ok(token_data.claims),
            Err(e) => {
                error!("Failed to decode JWT: {}", e);
                Err(format!("Invalid JWT token: {}", e))
            }
        }
    }
}

/// Verify JWT token signature using JWKS or static key
async fn verify_jwt_signature(token: &str) -> Result<Claims, String> {
    // First, try static key if available (for simple HMAC tokens)
    if let Ok(static_key) = std::env::var("JWT_SIGNING_KEY") {
        return verify_with_static_key(token, &static_key);
    }

    // Otherwise, use JWKS endpoint
    let jwks_url = if let Ok(url) = std::env::var("JWKS_URL") {
        url
    } else if let Ok(issuer) = std::env::var("JWT_ISSUER") {
        // Simple auto-derivation: add /.well-known/jwks.json to issuer
        format!("{}/.well-known/jwks.json", issuer.trim_end_matches('/'))
    } else {
        return Err("No JWT verification configuration found. Set one of: JWT_SIGNING_KEY (for HMAC), JWKS_URL (for RSA), or JWT_ISSUER (auto-derives JWKS URL)".to_string());
    };

    verify_with_jwks(token, &jwks_url).await
}

/// Verify JWT with static HMAC key
fn verify_with_static_key(token: &str, key: &str) -> Result<Claims, String> {
    let mut validation = Validation::new(Algorithm::HS256);

    // Configure audience - we'll validate this explicitly after decoding
    validation.validate_aud = false;
    let expected_aud =
        std::env::var("JWT_AUDIENCE").unwrap_or_else(|_| "infraweave-api".to_string());
    debug!(
        "JWT audience will be validated explicitly for: {}",
        expected_aud
    );

    // Configure issuer validation only if explicitly set
    if let Ok(expected_iss) = std::env::var("JWT_ISSUER") {
        validation.set_issuer(std::slice::from_ref(&expected_iss));
        debug!("JWT issuer validation enabled for: {}", expected_iss);
    } else {
        debug!("JWT issuer validation disabled - JWT_ISSUER not set");
    }

    let decoding_key = DecodingKey::from_secret(key.as_bytes());

    log::info!(
        "Attempting to decode JWT with audience validation for: {}",
        expected_aud
    );
    match decode::<Claims>(token, &decoding_key, &validation) {
        Ok(token_data) => {
            log::info!(
                "JWT decode successful. Claims: aud={:?}, sub={:?}",
                token_data.claims.aud,
                token_data.claims.sub
            );

            // Explicitly check that aud claim exists and matches expected value
            match &token_data.claims.aud {
                Some(token_aud) if token_aud == &expected_aud => {
                    log::info!(
                        "Audience validation passed: {} matches expected {}",
                        token_aud,
                        expected_aud
                    );
                    Ok(token_data.claims)
                }
                Some(token_aud) => {
                    error!(
                        "Audience validation failed: token has '{}', expected '{}'",
                        token_aud, expected_aud
                    );
                    Err(format!(
                        "JWT audience validation failed: expected '{}', got '{}'",
                        expected_aud, token_aud
                    ))
                }
                None => {
                    error!(
                        "Audience validation failed: JWT token missing 'aud' claim, expected '{}'",
                        expected_aud
                    );
                    Err(format!(
                        "JWT token missing required 'aud' claim, expected '{}'",
                        expected_aud
                    ))
                }
            }
        }
        Err(e) => {
            error!("JWT validation failed: {}", e);
            Err(format!("JWT validation failed: {}", e))
        }
    }
}

/// Verify JWT token against JWKS endpoint
async fn verify_with_jwks(token: &str, jwks_url: &str) -> Result<Claims, String> {
    // Decode header to get key ID
    let header = decode_header(token).map_err(|e| format!("Failed to decode JWT header: {}", e))?;

    let kid = header.kid.ok_or("JWT header missing key ID (kid)")?;

    // Fetch JWKS
    let jwks = fetch_jwks(jwks_url)
        .await
        .map_err(|e| format!("Failed to fetch JWKS: {}", e))?;

    // Find the key with matching kid
    let key = jwks
        .keys
        .iter()
        .find(|k| k.kid == kid)
        .ok_or(format!("Key with kid '{}' not found in JWKS", kid))?;

    // Convert key to DecodingKey
    let decoding_key =
        convert_jwk_to_decoding_key(key).map_err(|e| format!("Failed to convert JWK: {}", e))?;

    // Set up validation parameters
    let mut validation = Validation::new(Algorithm::RS256);

    // Configure audience - we'll validate this explicitly after decoding
    validation.validate_aud = false;
    let expected_aud =
        std::env::var("JWT_AUDIENCE").unwrap_or_else(|_| "infraweave-api".to_string());
    debug!(
        "JWT audience will be validated explicitly for: {}",
        expected_aud
    );

    // Configure issuer validation only if explicitly set
    if let Ok(expected_iss) = std::env::var("JWT_ISSUER") {
        validation.set_issuer(std::slice::from_ref(&expected_iss));
        debug!("JWT issuer validation enabled for: {}", expected_iss);
    } else {
        debug!("JWT issuer validation disabled - JWT_ISSUER not set");
    }

    // Decode and verify token
    match decode::<Claims>(token, &decoding_key, &validation) {
        Ok(token_data) => {
            // Explicitly check that aud claim exists and matches expected value
            match &token_data.claims.aud {
                Some(token_aud) if token_aud == &expected_aud => {
                    debug!(
                        "Audience validation passed: {} matches expected {}",
                        token_aud, expected_aud
                    );
                    Ok(token_data.claims)
                }
                Some(token_aud) => {
                    error!(
                        "Audience validation failed: token has '{}', expected '{}'",
                        token_aud, expected_aud
                    );
                    Err(format!(
                        "JWT audience validation failed: expected '{}', got '{}'",
                        expected_aud, token_aud
                    ))
                }
                None => {
                    error!(
                        "Audience validation failed: JWT token missing 'aud' claim, expected '{}'",
                        expected_aud
                    );
                    Err(format!(
                        "JWT token missing required 'aud' claim, expected '{}'",
                        expected_aud
                    ))
                }
            }
        }
        Err(e) => Err(format!("JWT validation failed: {}", e)),
    }
}

/// Fetch JWKS from endpoint
async fn fetch_jwks(url: &str) -> Result<Jwks, Box<dyn std::error::Error + Send + Sync>> {
    let client = reqwest::Client::new();
    let response = client.get(url).send().await?;
    let jwks: Jwks = response.json().await?;
    Ok(jwks)
}

/// Convert JWK to jsonwebtoken's DecodingKey
fn convert_jwk_to_decoding_key(key: &JwksKey) -> Result<DecodingKey, String> {
    // For RSA keys, we need the x5c (certificate) or n/e (modulus/exponent)
    if let Some(x5c) = &key.x5c {
        if let Some(cert) = x5c.first() {
            // Decode base64 certificate
            let cert_der = general_purpose::STANDARD
                .decode(cert)
                .map_err(|e| format!("Failed to decode certificate: {}", e))?;

            // Create decoding key from certificate
            // Note: from_rsa_der returns DecodingKey directly, not a Result in jsonwebtoken 9.0
            let decoding_key = DecodingKey::from_rsa_der(&cert_der);
            return Ok(decoding_key);
        }
    }

    // Alternative: use n and e for RSA
    if let (Some(_n), Some(_e)) = (&key.n, &key.e) {
        // This is more complex and requires additional crypto libraries
        // For now, we'll return an error and rely on x5c
        return Err("RSA key reconstruction from n/e not implemented".to_string());
    }

    Err("Unsupported key format".to_string())
}

/// Get user identifier from JWT claims with fallback strategy
fn get_user_identifier(claims: &Claims) -> Option<String> {
    // Try common user identifier claims in order of preference
    if let Some(sub) = &claims.sub {
        debug!("Using Subject (sub) as user identifier: {}", sub);
        return Some(sub.clone());
    }

    // Check custom claims for other common user identifiers
    for key in &["oid", "user_id", "username", "email", "upn", "appid"] {
        if let Some(value) = claims.custom.get(*key) {
            if let Some(user_id) = value.as_str() {
                return Some(user_id.to_string());
            }
        }
    }

    warn!("No user identifier found in JWT claims");
    None
}

/// Extract projects from JWT claims using the configured claim key
fn extract_projects_from_claims(claims: &Claims) -> Vec<String> {
    let project_claim_key = std::env::var("JWT_PROJECT_CLAIM_KEY")
        .expect("JWT_PROJECT_CLAIM_KEY environment variable is required");

    if let Some(claim_value) = claims.custom.get(&project_claim_key) {
        extract_projects_from_claim_value(claim_value, &project_claim_key)
    } else {
        debug!("Project claim key '{}' not found in JWT", project_claim_key);
        Vec::new()
    }
}

/// Extract string array from JSON claim value
fn extract_projects_from_claim_value(value: &serde_json::Value, claim_key: &str) -> Vec<String> {
    match value {
        serde_json::Value::Array(arr) => {
            let projects: Vec<String> = arr
                .iter()
                .filter_map(|item| item.as_str().map(|s| s.to_string()))
                .collect();
            debug!(
                "Extracted {} projects from '{}' claim array",
                projects.len(),
                claim_key
            );
            projects
        }
        serde_json::Value::String(s) => {
            if !s.is_empty() {
                debug!(
                    "Extracted single project '{}' from '{}' claim",
                    s, claim_key
                );
                vec![s.to_string()]
            } else {
                debug!("Empty project string in '{}' claim", claim_key);
                Vec::new()
            }
        }
        _ => {
            debug!(
                "'{}' claim is not a string or array: {:?}",
                claim_key, value
            );
            Vec::new()
        }
    }
}

/// Middleware to validate project access based on JWT token and project parameter
pub async fn project_access_middleware(
    mut request: Request,
    next: Next,
) -> Result<Response, Response> {
    // Check for internal MCP token first (highest priority - process-isolated auth)
    if let Some(internal_token) = get_internal_token() {
        let auth_header = request
            .headers()
            .get("Authorization")
            .and_then(|h| h.to_str().ok());

        if let Some(header) = auth_header {
            let token = header.trim_start_matches("Bearer ").trim();
            if token == internal_token {
                log::debug!(
                    "Internal token authentication successful - MCP process access granted"
                );
                return Ok(next.run(request).await);
            }
        }
    }

    // Check if JWT authentication is completely disabled (INSECURE!)
    let disable_jwt_auth = std::env::var("DISABLE_JWT_AUTH_INSECURE")
        .unwrap_or_default()
        .to_lowercase()
        == "true";

    if disable_jwt_auth {
        warn!("JWT authentication middleware bypassed - INSECURE! Only use in development!");
        return Ok(next.run(request).await);
    }

    // Extract Authorization header
    let auth_header = request
        .headers()
        .get("Authorization")
        .and_then(|h| h.to_str().ok());

    let auth_header = match auth_header {
        Some(header) => header,
        None => {
            let error_response = Json(json!({
                "error": "Missing Authorization header",
                "code": "MISSING_AUTH"
            }));
            return Err((StatusCode::UNAUTHORIZED, error_response).into_response());
        }
    };

    // Extract and validate JWT
    let claims = match extract_and_validate_jwt(auth_header).await {
        Ok(claims) => claims,
        Err(e) => {
            let error_response = Json(json!({
                "error": format!("Authentication failed: {}", e),
                "code": "INVALID_TOKEN"
            }));
            return Err((StatusCode::UNAUTHORIZED, error_response).into_response());
        }
    };

    // Get user identifier from claims
    let user_identifier = match get_user_identifier(&claims) {
        Some(id) => id,
        None => {
            let error_response = Json(json!({
                "error": "No user identifier found in token",
                "code": "MISSING_USER_ID",
                "details": "Token must contain sub, oid, user_id, username, email, upn, or appid claim"
            }));
            return Err((StatusCode::UNAUTHORIZED, error_response).into_response());
        }
    };

    // Extract project ID from the request path
    let uri_path = request.uri().path();
    let uri_path_str = uri_path.to_string(); // Store for later logging
    let project_id = extract_project_id_from_path(uri_path);

    let project_id = match project_id {
        Some(id) => id,
        None => {
            debug!("No project ID found in path: {}", uri_path);
            // For routes that don't require project access, continue without validation
            return Ok(next.run(request).await);
        }
    };

    // Extract projects from JWT claims using the configured claim key
    let accessible_projects = extract_projects_from_claims(&claims);

    // Validate that the user has access to the requested project
    if !validate_project_access(&project_id, &accessible_projects) {
        let error_response = Json(json!({
            "error": format!("Access denied to project: {}", project_id),
            "code": "PROJECT_ACCESS_DENIED",
            "project_id": project_id,
            "user_id": user_identifier,
            "accessible_projects": accessible_projects
        }));
        return Err((StatusCode::FORBIDDEN, error_response).into_response());
    }

    // Store the project access context in request extensions for use by handlers
    let context = ProjectAccessContext {
        project_id: project_id.clone(),
        user_id: user_identifier.clone(),
    };
    request.extensions_mut().insert(context);

    // Log successful authentication and project access
    log::info!(
        "Authenticated access granted - Project: {}, Path: {}, Accessible Projects: {:?}",
        project_id,
        uri_path_str,
        accessible_projects
    );

    Ok(next.run(request).await)
}

/// Extract project ID from the request path
/// Handles paths like: /api/v1/deployment/{project}/{region}/{environment}/{deployment_id}
fn extract_project_id_from_path(path: &str) -> Option<String> {
    let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();

    // Look for patterns where project ID is expected
    match segments.as_slice() {
        // Routes that don't require project access (check these first)
        ["api", "v1", "modules"] => None,
        ["api", "v1", "projects"] => None,
        ["api", "v1", "stacks"] => None,
        ["api", "v1", "module", ..] => None,
        ["api", "v1", "stack", ..] => None,
        ["api", "v1", "policy", ..] => None,
        ["api", "v1", "policies", _] => None,
        ["api", "v1", "modules", "versions", ..] => None,
        ["api", "v1", "stacks", "versions", ..] => None,

        // Project-specific routes (check specific patterns before general ones)
        ["api", "v1", "deployments", "module", project, ..] => Some(project.to_string()),
        ["api", "v1", "deployment", project, ..] => Some(project.to_string()),
        ["api", "v1", "deployments", project, ..] => Some(project.to_string()),
        ["api", "v1", "logs", project, ..] => Some(project.to_string()),
        ["api", "v1", "events", project, ..] => Some(project.to_string()),
        ["api", "v1", "change_record", project, ..] => Some(project.to_string()),

        _ => None,
    }
}

/// Validate that a user has access to a specific project
fn validate_project_access(project_id: &str, accessible_projects: &[String]) -> bool {
    // Check for direct match
    for accessible_project in accessible_projects {
        if project_id == accessible_project {
            debug!(
                "Project access granted: {} matches {}",
                project_id, accessible_project
            );
            return true;
        }
    }

    warn!(
        "Project access denied: project={}, accessible_projects={:?}",
        project_id, accessible_projects
    );
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn test_extract_project_id_from_path() {
        assert_eq!(
            extract_project_id_from_path("/api/v1/deployment/project123/region/env/deploy1"),
            Some("project123".to_string())
        );

        assert_eq!(
            extract_project_id_from_path("/api/v1/deployments/project456/region"),
            Some("project456".to_string())
        );

        assert_eq!(extract_project_id_from_path("/api/v1/modules"), None);

        assert_eq!(extract_project_id_from_path("/api/v1/projects"), None);
    }

    #[test]
    fn test_validate_project_access() {
        // Direct match
        assert!(validate_project_access(
            "project123",
            &vec!["project123".to_string()]
        ));

        // No prefix matching - exact match only
        assert!(!validate_project_access(
            "project123-dev",
            &vec!["project123".to_string()]
        ));

        // Multiple accessible projects
        assert!(validate_project_access(
            "project456",
            &vec!["project123".to_string(), "project456".to_string()]
        ));

        // No match
        assert!(!validate_project_access(
            "project123",
            &vec!["project456".to_string()]
        ));

        // Empty access list
        assert!(!validate_project_access("project123", &vec![]));
    }

    #[test]
    fn test_extract_projects_from_claim_value() {
        // Array of project names
        let projects = serde_json::json!(["project1", "project2", "project3"]);
        let result = extract_projects_from_claim_value(&projects, "projects");
        assert_eq!(result, vec!["project1", "project2", "project3"]);

        // Single project string
        let single_project = serde_json::json!("project1");
        let result = extract_projects_from_claim_value(&single_project, "projects");
        assert_eq!(result, vec!["project1"]);

        // Array with mixed content (only strings are extracted)
        let mixed = serde_json::json!(["project1", 42, "project2", null, "project3"]);
        let result = extract_projects_from_claim_value(&mixed, "projects");
        assert_eq!(result, vec!["project1", "project2", "project3"]);

        // Empty string
        let empty_string = serde_json::json!("");
        let result = extract_projects_from_claim_value(&empty_string, "projects");
        assert!(result.is_empty());

        // Invalid format
        let invalid = serde_json::json!(42);
        let result = extract_projects_from_claim_value(&invalid, "projects");
        assert!(result.is_empty());
    }

    #[tokio::test]
    async fn test_jwt_authentication_flow() {
        use jsonwebtoken::{encode, EncodingKey, Header};
        use std::collections::HashMap;

        // Set up test environment variables
        std::env::set_var("JWT_PROJECT_CLAIM_KEY", "projects");
        std::env::set_var("DISABLE_JWT_AUTH_INSECURE", "true"); // Disable auth for test
        std::env::remove_var("JWT_AUDIENCE"); // Remove audience validation
        std::env::remove_var("JWT_ISSUER"); // Remove issuer validation

        // Create test JWT claims
        let mut custom_claims = HashMap::new();
        custom_claims.insert(
            "projects".to_string(),
            serde_json::json!(["infraweave-dev", "infraweave-prod", "test-project"]),
        );

        let claims = Claims {
            sub: Some("user123".to_string()),
            iss: Some("https://auth.infraweave.io".to_string()),
            aud: Some("infraweave-api".to_string()),
            exp: Some((chrono::Utc::now() + chrono::Duration::hours(1)).timestamp() as usize),
            iat: Some(chrono::Utc::now().timestamp() as usize),
            custom: custom_claims,
        };

        // Create JWT token
        let header = Header::default();
        let encoding_key = EncodingKey::from_secret(b"test-secret");
        let token = encode(&header, &claims, &encoding_key).unwrap();
        let auth_header = format!("Bearer {}", token);

        // Test JWT extraction and validation
        let extracted_claims = extract_and_validate_jwt(&auth_header).await.unwrap();

        // Verify user identifier extraction
        let user_id = get_user_identifier(&extracted_claims).unwrap();
        assert_eq!(user_id, "user123");

        // Verify project extraction
        let projects = extract_projects_from_claims(&extracted_claims);
        assert_eq!(
            projects,
            vec!["infraweave-dev", "infraweave-prod", "test-project"]
        );

        // Test project access validation
        assert!(validate_project_access("infraweave-dev", &projects));
        assert!(validate_project_access("infraweave-prod", &projects));
        assert!(validate_project_access("test-project", &projects));
        assert!(!validate_project_access("unauthorized-project", &projects));

        // Test prefix match (should not match)
        assert!(!validate_project_access(
            "infraweave-dev-staging",
            &projects
        ));

        // Clean up environment
        std::env::remove_var("JWT_PROJECT_CLAIM_KEY");
        std::env::remove_var("DISABLE_JWT_AUTH_INSECURE");
    }

    #[test]
    fn test_get_user_identifier_fallback() {
        let mut custom_claims = HashMap::new();

        // Test with standard 'sub' claim
        let claims_with_sub = Claims {
            sub: Some("user-from-sub".to_string()),
            iss: None,
            aud: None,
            exp: None,
            iat: None,
            custom: custom_claims.clone(),
        };
        assert_eq!(
            get_user_identifier(&claims_with_sub),
            Some("user-from-sub".to_string())
        );

        // Test fallback to 'oid' (Azure AD)
        custom_claims.insert("oid".to_string(), serde_json::json!("user-from-oid"));
        let claims_with_oid = Claims {
            sub: None,
            iss: None,
            aud: None,
            exp: None,
            iat: None,
            custom: custom_claims.clone(),
        };
        assert_eq!(
            get_user_identifier(&claims_with_oid),
            Some("user-from-oid".to_string())
        );

        // Test fallback to email
        custom_claims.clear();
        custom_claims.insert("email".to_string(), serde_json::json!("user@example.com"));
        let claims_with_email = Claims {
            sub: None,
            iss: None,
            aud: None,
            exp: None,
            iat: None,
            custom: custom_claims,
        };
        assert_eq!(
            get_user_identifier(&claims_with_email),
            Some("user@example.com".to_string())
        );

        // Test no identifier found
        let claims_empty = Claims {
            sub: None,
            iss: None,
            aud: None,
            exp: None,
            iat: None,
            custom: HashMap::new(),
        };
        assert_eq!(get_user_identifier(&claims_empty), None);
    }
}
