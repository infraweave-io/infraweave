use anyhow::{anyhow, Context, Result};
use colored::Colorize;
use env_utils::config_path::get_token_path;
use rand::Rng;
use serde::{Deserialize, Serialize};
use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::sync::mpsc;
use std::thread;

const CALLBACK_PATH: &str = "/callback";
const DEFAULT_CALLBACK_PORT: u16 = 19847;
const AUTH_TIMEOUT_SECS: u64 = 300;

#[derive(Debug, Deserialize)]
struct TokenResponse {
    sign_in_url: Option<String>,
    access_token: Option<String>,
    id_token: Option<String>,
    refresh_token: Option<String>,
    #[serde(default)]
    success: bool,
    message: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct StoredTokens {
    access_token: String,
    id_token: String,
    refresh_token: Option<String>,
    expires_at: Option<i64>,
    api_endpoint: String,
    #[serde(default)]
    region: Option<String>,
}

/// Execute login flow: get sign-in URL, open browser, catch callback, exchange code for tokens
pub async fn execute_login(api_endpoint: String) -> Result<()> {
    println!("{}", "Starting OAuth login flow...".bright_blue());

    // Step 1: Get the sign-in URL from the API
    println!("Requesting sign-in URL...");
    let client = reqwest::Client::new();

    let token_url = format!("{}/api/v1/auth/token", api_endpoint.trim_end_matches('/'));

    let is_local_endpoint =
        api_endpoint.contains("127.0.0.1") || api_endpoint.contains("localhost");

    // Check cloud provider credentials availability
    let (has_credentials, mut region) = match http_client::http_auth::get_auth_context().await {
        Ok(ctx) => ctx,
        Err(e) => {
            if is_local_endpoint {
                return store_local_tokens(&api_endpoint);
            }
            return Err(e).context("Failed to check cloud credentials");
        }
    };

    if !has_credentials {
        if is_local_endpoint {
            return store_local_tokens(&api_endpoint);
        }
        return Err(anyhow!(
            "No cloud credentials found. Configure credentials or use a local endpoint."
        ));
    }

    // Always attempt to discover region from the API metadata
    // This supports both standard regional endpoints and multi-region custom domains
    println!(
        "{}",
        "Discovering API region from metadata endpoint...".dimmed()
    );
    let meta_url = format!("{}/api/v1/meta", api_endpoint.trim_end_matches('/'));

    // NOTE: The /meta endpoint MUST be unauthenticated because we cannot sign the request
    // until we know which region we are talking to (bootstrap problem).
    match client
        .get(&meta_url)
        .timeout(std::time::Duration::from_secs(3))
        .send()
        .await
    {
        Ok(resp) => {
            if resp.status().is_success() {
                if let Ok(json) = resp.json::<serde_json::Value>().await {
                    if let Some(r) = json.get("region").and_then(|v| v.as_str()) {
                        if r != region {
                            println!(
                                "{}",
                                format!("✓ Discovered region '{}' from metadata", r).green()
                            );
                            region = r.to_string();
                        } else {
                            println!(
                                "{}",
                                format!("✓ Confirmed region '{}' from metadata", r).green()
                            );
                        }
                    }
                }
            } else {
                println!(
                    "{}",
                    format!(
                        "Warning: Metadata endpoint returned status {}. Proceeding with configured region: {}",
                        resp.status(),
                        region
                    )
                    .yellow()
                );
            }
        }
        Err(e) => {
            println!(
                "{}",
                format!(
                    "Warning: Failed to discover region metadata ({}). Proceeding with configured region: {}",
                    e, region
                )
                .yellow()
            );
        }
    }

    // Step 2: Bind the callback server on a fixed port (must match the IdP's registered
    // redirect URI). Override with INFRAWEAVE_CALLBACK_PORT if needed.
    let callback_port: u16 = std::env::var("INFRAWEAVE_CALLBACK_PORT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(DEFAULT_CALLBACK_PORT);

    let (tx, rx) = mpsc::channel();
    let listener = TcpListener::bind(format!("127.0.0.1:{}", callback_port)).context(format!(
        "Failed to bind to localhost:{}. Is another process using this port?",
        callback_port
    ))?;
    let redirect_uri = format!("http://localhost:{}{}", callback_port, CALLBACK_PATH);

    println!(
        "{}",
        format!(
            "Starting local server on http://localhost:{}...",
            callback_port
        )
        .bright_blue()
    );

    // Generate PKCE code verifier and challenge (RFC 7636)
    let code_verifier = generate_pkce_verifier();
    let code_challenge = generate_pkce_challenge(&code_verifier);

    // Make the first request to get the sign-in URL
    let body = serde_json::json!({
        "redirect_uri": redirect_uri,
        "code_challenge": code_challenge,
        "code_challenge_method": "S256",
    });
    let (status, response_text) = http_client::http_auth::call_authenticated_http_raw(
        "POST",
        &token_url,
        Some(body),
        Some(&region),
    )
    .await
    .context("Failed to request sign-in URL")?;

    if !(200..300).contains(&status) {
        return Err(anyhow!(
            "API request failed with status {} (region: {}): {}",
            status,
            region,
            response_text
        ));
    }

    let token_response: TokenResponse = serde_json::from_str(&response_text).context(format!(
        "Failed to parse sign-in URL response. Response body: {}",
        response_text
    ))?;

    if !token_response.success {
        return Err(anyhow!(
            "Failed to get sign-in URL: {}",
            token_response.message.unwrap_or_default()
        ));
    }

    let sign_in_url = token_response
        .sign_in_url
        .ok_or_else(|| anyhow!("No sign-in URL in response"))?;

    println!("{} {}", "Sign-in URL:".green(), sign_in_url);

    // Generate a random state parameter to prevent CSRF attacks
    let oauth_state: String = {
        let mut rng = rand::thread_rng();
        (0..32)
            .map(|_| {
                let idx = rng.gen_range(0..36);
                if idx < 10 {
                    (b'0' + idx) as char
                } else {
                    (b'a' + idx - 10) as char
                }
            })
            .collect()
    };
    let expected_state = oauth_state.clone();

    // Spawn server thread — loop to handle preflight/favicon requests until code received
    thread::spawn(move || {
        // Set a timeout so the thread doesn't hang forever if the main thread gives up
        listener
            .set_nonblocking(false)
            .expect("Cannot set blocking");
        let deadline =
            std::time::Instant::now() + std::time::Duration::from_secs(AUTH_TIMEOUT_SECS + 10);

        while std::time::Instant::now() < deadline {
            let (mut stream, _) = match listener.accept() {
                Ok(conn) => conn,
                Err(_) => break,
            };

            let buf_reader = BufReader::new(&stream);
            let request_line = buf_reader.lines().next();

            let line = match request_line {
                Some(Ok(l)) => l,
                _ => continue,
            };

            // Validate the request path matches CALLBACK_PATH
            let expected_prefix = format!("GET {}", CALLBACK_PATH);
            if !line.starts_with(&expected_prefix) {
                let body = "<html><body><h1>Not Found</h1></body></html>";
                let response = format!("HTTP/1.1 404 Not Found\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body);
                let _ = stream.write_all(response.as_bytes());
                continue;
            }

            // Parse query parameters from the request line
            let (query_start, http_start) = match (line.find('?'), line.find(" HTTP/")) {
                (Some(q), Some(h)) => (q, h),
                _ => {
                    let body = "<html><body><h1>Error</h1><p>No authorization code found.</p></body></html>";
                    let response = format!("HTTP/1.1 400 Bad Request\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body);
                    let _ = stream.write_all(response.as_bytes());
                    continue;
                }
            };

            let query = &line[query_start + 1..http_start];
            let params: std::collections::HashMap<String, String> =
                url::form_urlencoded::parse(query.as_bytes())
                    .map(|(k, v)| (k.into_owned(), v.into_owned()))
                    .collect();

            // Validate state parameter to prevent CSRF
            match params.get("state") {
                Some(state) if state == &expected_state => {}
                _ => {
                    let body = "<html><body><h1>Error</h1><p>Invalid or missing state parameter.</p></body></html>";
                    let response = format!("HTTP/1.1 400 Bad Request\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body);
                    let _ = stream.write_all(response.as_bytes());
                    continue;
                }
            }

            if let Some(code) = params.get("code") {
                let body = "<html><body><h1>Login Successful!</h1><p>You can close this window and return to the CLI.</p></body></html>";
                let response = format!("HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body);
                let _ = stream.write_all(response.as_bytes());
                let _ = tx.send(code.clone());
                return;
            }

            let body =
                "<html><body><h1>Error</h1><p>No authorization code found.</p></body></html>";
            let response = format!("HTTP/1.1 400 Bad Request\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body);
            let _ = stream.write_all(response.as_bytes());
        }
    });

    // Step 3: Open browser (append state to the sign-in URL)
    let sign_in_url_with_state = if sign_in_url.contains('?') {
        format!("{}&state={}", sign_in_url, oauth_state)
    } else {
        format!("{}?state={}", sign_in_url, oauth_state)
    };

    println!("{}", "Opening browser...".bright_blue());
    if let Err(e) = webbrowser::open(&sign_in_url_with_state) {
        eprintln!(
            "{} {}",
            "Warning: Could not open browser automatically:".yellow(),
            e
        );
        println!("\n{}", "Please open this URL manually:".yellow());
        println!("{}\n", sign_in_url_with_state.bright_cyan());
    }

    // Step 4: Wait for callback
    println!("{}", "Waiting for authentication callback...".bright_blue());
    let code = rx
        .recv_timeout(std::time::Duration::from_secs(AUTH_TIMEOUT_SECS))
        .context("Timeout waiting for authentication (5 minutes)")?;

    println!("{}", "✓ Authorization code received".green());

    // Step 5: Exchange code for tokens
    println!("Exchanging code for tokens...");

    let exchange_body = serde_json::json!({
        "code": code,
        "redirect_uri": redirect_uri,
        "code_verifier": code_verifier,
    });

    let (status, response_text) = http_client::http_auth::call_authenticated_http_raw(
        "POST",
        &token_url,
        Some(exchange_body),
        Some(&region),
    )
    .await
    .context("Failed to exchange code for tokens")?;

    if !(200..300).contains(&status) {
        return Err(anyhow!(
            "Token exchange failed with status {}: {}",
            status,
            response_text
        ));
    }

    let token_response: TokenResponse = serde_json::from_str(&response_text).context(format!(
        "Failed to parse token response. Response body: {}",
        response_text
    ))?;

    if !token_response.success {
        return Err(anyhow!(
            "Failed to exchange code for tokens: {}",
            token_response.message.unwrap_or_default()
        ));
    }

    let access_token = token_response
        .access_token
        .ok_or_else(|| anyhow!("No access token in response"))?;
    let id_token = token_response
        .id_token
        .ok_or_else(|| anyhow!("No ID token in response"))?;

    // Parse JWT to get expiration time
    let expires_at = extract_jwt_expiration(&access_token).ok();

    // Step 6: Store tokens
    let tokens = StoredTokens {
        access_token: access_token.clone(),
        id_token: id_token.clone(),
        refresh_token: token_response.refresh_token,
        expires_at,
        api_endpoint: api_endpoint.clone(),
        region: Some(region),
    };

    store_tokens(&tokens)?;

    println!("{}", "✓ Login successful! Tokens stored.".green());
    println!(
        "{}",
        format!("Token file: {}", get_token_path()?.display()).dimmed()
    );
    println!(
        "{}",
        format!("API endpoint configured: {}", api_endpoint).dimmed()
    );

    Ok(())
}

/// Store tokens to disk
fn store_tokens(tokens: &StoredTokens) -> Result<()> {
    let path = env_utils::config_path::get_token_path()?;
    let json = serde_json::to_string_pretty(tokens)?;
    std::fs::write(&path, &json).context("Failed to write tokens to file")?;

    // Restrict file permissions to owner-only (0600) on Unix
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))
            .context("Failed to set token file permissions")?;
    }

    // TODO: On Windows, consider restricting ACLs to the current user.
    // The default inherited ACLs from the user profile directory are
    // typically sufficient, but explicit restriction would be ideal.

    Ok(())
}

/// Store tokens for local/unauthenticated mode
fn store_local_tokens(api_endpoint: &str) -> Result<()> {
    println!(
        "{}",
        "No cloud credentials needed for local endpoint. Storing API endpoint for local mode."
            .yellow()
    );
    let tokens = StoredTokens {
        access_token: "local".to_string(),
        id_token: "local".to_string(),
        refresh_token: None,
        expires_at: None,
        api_endpoint: api_endpoint.to_string(),
        region: None,
    };
    store_tokens(&tokens)?;
    println!("{}", "✓ API endpoint configured.".green());
    println!("{}", format!("API endpoint: {}", api_endpoint).dimmed());
    Ok(())
}

/// Load tokens from disk
fn load_tokens() -> Result<StoredTokens> {
    let path = env_utils::config_path::get_token_path()?;
    let json = std::fs::read_to_string(&path).context("Failed to read tokens file")?;
    let tokens: StoredTokens = serde_json::from_str(&json).context("Failed to parse tokens")?;
    Ok(tokens)
}

/// Check if user is logged in (has non-expired tokens)
pub fn is_logged_in() -> bool {
    match load_tokens() {
        Ok(tokens) => {
            if let Some(expires_at) = tokens.expires_at {
                let now = chrono::Utc::now().timestamp();
                return now < expires_at;
            }
            true
        }
        Err(_) => false,
    }
}

/// Get the current ID token (for use in API calls)
pub fn get_id_token() -> Result<String> {
    let tokens = load_tokens()?;

    // Check if the token has expired
    if let Some(expires_at) = tokens.expires_at {
        let now = chrono::Utc::now().timestamp();
        if now >= expires_at {
            return Err(anyhow!(
                "Token has expired. Please run `infraweave login` to re-authenticate."
            ));
        }
    }

    Ok(tokens.id_token)
}

/// Get the current ID token, attempting a refresh if the token has expired.
pub async fn get_id_token_or_refresh() -> Result<String> {
    match get_id_token() {
        Ok(token) => Ok(token),
        Err(_) => {
            // Attempt to refresh before giving up
            attempt_token_refresh().await?;
            get_id_token()
        }
    }
}

/// Attempt to refresh tokens using the stored refresh_token.
async fn attempt_token_refresh() -> Result<()> {
    let tokens = load_tokens()
        .context("No stored tokens found. Please run `infraweave login` to authenticate.")?;

    let refresh_token = tokens.refresh_token.ok_or_else(|| {
        anyhow!("No refresh token available. Please run `infraweave login` to re-authenticate.")
    })?;

    println!("{}", "Token expired, attempting refresh...".bright_blue());

    let token_url = format!(
        "{}/api/v1/auth/token",
        tokens.api_endpoint.trim_end_matches('/')
    );

    let body = serde_json::json!({
        "grant_type": "refresh_token",
        "refresh_token": refresh_token,
    });

    // Use the region stored at login time; fall back to get_auth_context if
    // the token file predates the region field.
    let region = match tokens.region {
        Some(ref r) => r.clone(),
        None => {
            let (_has_credentials, r) = http_client::http_auth::get_auth_context()
                .await
                .context("Failed to get auth context for token refresh")?;
            r
        }
    };

    let (status, response_text) = http_client::http_auth::call_authenticated_http_raw(
        "POST",
        &token_url,
        Some(body),
        Some(&region),
    )
    .await
    .context("Failed to call token refresh endpoint")?;

    if !(200..300).contains(&status) {
        return Err(anyhow!(
            "Token refresh failed (status {}). Please run `infraweave login` to re-authenticate.",
            status
        ));
    }

    let token_response: TokenResponse =
        serde_json::from_str(&response_text).context("Failed to parse refresh response")?;

    if !token_response.success {
        return Err(anyhow!(
            "Token refresh failed: {}. Please run `infraweave login` to re-authenticate.",
            token_response.message.unwrap_or_default()
        ));
    }

    let access_token = token_response
        .access_token
        .ok_or_else(|| anyhow!("No access token in refresh response"))?;
    let id_token = token_response
        .id_token
        .ok_or_else(|| anyhow!("No ID token in refresh response"))?;

    let expires_at = extract_jwt_expiration(&access_token).ok();

    let new_tokens = StoredTokens {
        access_token,
        id_token,
        refresh_token: token_response.refresh_token.or(Some(refresh_token.clone())),
        expires_at,
        api_endpoint: tokens.api_endpoint,
        region: Some(region),
    };

    store_tokens(&new_tokens)?;
    println!("{}", "✓ Token refreshed successfully.".green());
    Ok(())
}

/// Generate a PKCE code verifier (RFC 7636 §4.1).
/// 32 random bytes → 43-char base64url string (no padding).
fn generate_pkce_verifier() -> String {
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
    let mut buf = [0u8; 32];
    rand::Rng::fill(&mut rand::thread_rng(), &mut buf);
    URL_SAFE_NO_PAD.encode(buf)
}

/// Derive the PKCE code challenge from a verifier (RFC 7636 §4.2).
/// challenge = BASE64URL(SHA256(verifier))
fn generate_pkce_challenge(verifier: &str) -> String {
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
    let digest = ring::digest::digest(&ring::digest::SHA256, verifier.as_bytes());
    URL_SAFE_NO_PAD.encode(digest.as_ref())
}

/// Extract expiration time from JWT token
fn extract_jwt_expiration(token: &str) -> Result<i64> {
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};

    // JWT format: header.payload.signature
    let parts: Vec<&str> = token.split('.').collect();
    if parts.len() != 3 {
        return Err(anyhow!("Invalid JWT format"));
    }

    // Decode the payload (second part) - JWTs use URL-safe base64 without padding.
    // NOTE: We intentionally skip signature verification here. This function is only
    // used to read the `exp` claim for local expiry checks; the server validates the
    // full token signature on every API call.
    let payload_bytes = URL_SAFE_NO_PAD
        .decode(parts[1])
        .context("Failed to decode JWT payload")?;
    let payload_str = String::from_utf8(payload_bytes).context("JWT payload is not valid UTF-8")?;
    let payload: serde_json::Value =
        serde_json::from_str(&payload_str).context("Failed to parse JWT payload JSON")?;

    // Extract expiration time
    payload
        .get("exp")
        .and_then(|v| v.as_i64())
        .ok_or_else(|| anyhow!("No exp field in JWT"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};

    fn make_jwt(payload_json: &str) -> String {
        let header = URL_SAFE_NO_PAD.encode(r#"{"alg":"RS256","typ":"JWT"}"#);
        let payload = URL_SAFE_NO_PAD.encode(payload_json);
        let signature = URL_SAFE_NO_PAD.encode(b"fakesig");
        format!("{}.{}.{}", header, payload, signature)
    }

    #[test]
    fn test_extract_jwt_expiration_valid() {
        let token = make_jwt(r#"{"sub":"user","exp":1700000000}"#);
        let exp = extract_jwt_expiration(&token).unwrap();
        assert_eq!(exp, 1700000000);
    }

    #[test]
    fn test_extract_jwt_expiration_missing_exp() {
        let token = make_jwt(r#"{"sub":"user"}"#);
        let result = extract_jwt_expiration(&token);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("No exp field"));
    }

    #[test]
    fn test_extract_jwt_expiration_invalid_format() {
        assert!(extract_jwt_expiration("not.a.valid-base64!").is_err());
        assert!(extract_jwt_expiration("onlytwoparts.here").is_err());
        assert!(extract_jwt_expiration("").is_err());
    }

    #[test]
    fn test_store_and_load_tokens_round_trip() {
        // Use a temp dir to avoid touching the real config
        let dir = std::env::temp_dir().join(format!("infraweave_test_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let token_file = dir.join("tokens.json");

        let tokens = StoredTokens {
            access_token: "acc".into(),
            id_token: "idt".into(),
            refresh_token: Some("ref".into()),
            expires_at: Some(9999999999),
            api_endpoint: "http://localhost:9090".into(),
            region: Some("us-west-2".into()),
        };

        // Write directly to the temp path
        let json = serde_json::to_string_pretty(&tokens).unwrap();
        std::fs::write(&token_file, &json).unwrap();

        // Read back and verify
        let read_json = std::fs::read_to_string(&token_file).unwrap();
        let loaded: StoredTokens = serde_json::from_str(&read_json).unwrap();
        assert_eq!(loaded.access_token, "acc");
        assert_eq!(loaded.id_token, "idt");
        assert_eq!(loaded.refresh_token.as_deref(), Some("ref"));
        assert_eq!(loaded.expires_at, Some(9999999999));
        assert_eq!(loaded.api_endpoint, "http://localhost:9090");
        assert_eq!(loaded.region.as_deref(), Some("us-west-2"));

        // Clean up
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_stored_tokens_missing_optional_fields() {
        let json = r#"{
            "access_token": "a",
            "id_token": "i",
            "api_endpoint": "http://localhost"
        }"#;
        let tokens: StoredTokens = serde_json::from_str(json).unwrap();
        assert!(tokens.refresh_token.is_none());
        assert!(tokens.expires_at.is_none());
        assert!(tokens.region.is_none());
    }
}
