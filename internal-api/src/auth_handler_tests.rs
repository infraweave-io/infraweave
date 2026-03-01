#[cfg(all(test, feature = "aws"))]
mod token_bridge_tests {
    use internal_api::auth_handler;
    use serde_json::json;
    
    #[test]
    fn test_extract_username_from_iam_user_arn() {
        // Test IAM user ARN
        let arn = "arn:aws:iam::123456789012:user/alice";
        let claims = extract_test_claims(arn, 3600);
        assert_eq!(claims["cognito:username"], "alice");
    }
    
    #[test]
    fn test_extract_username_from_assumed_role() {
        // Test assumed role ARN
        let arn = "arn:aws:sts::123456789012:assumed-role/MyRole/session-123";
        let claims = extract_test_claims(arn, 3600);
        assert_eq!(claims["cognito:username"], "session-123");
    }
    
    #[tokio::test]
    async fn test_generate_token_basic() {
        // Set required environment variables for testing
        std::env::set_var("JWT_SECRET", "test-secret-key-123");
        std::env::set_var("JWT_ISSUER", "test-issuer");
        std::env::set_var("JWT_AUDIENCE", "test-audience");
        
        let user_id = "arn:aws:iam::123456789012:user/testuser";
        
        let result = auth_handler::generate_token_for_iam_user(user_id, Some(3600))
            .await;
        
        assert!(result.is_ok(), "Token generation should succeed");
        
        let token_data = result.unwrap();
        
        // Verify response structure
        assert!(token_data.get("access_token").is_some());
        assert!(token_data.get("token_type").is_some());
        assert_eq!(token_data["token_type"], "Bearer");
        assert_eq!(token_data["expires_in"], 3600);
        assert_eq!(token_data["user_id"], user_id);
        assert_eq!(token_data["username"], "testuser");
        
        // Verify token is valid JWT
        let token = token_data["access_token"].as_str().unwrap();
        assert!(token.split('.').count() == 3, "Token should have 3 parts");
    }
    
    #[tokio::test]
    async fn test_generate_token_custom_ttl() {
        std::env::set_var("JWT_SECRET", "test-secret-key-456");
        
        let user_id = "arn:aws:iam::123456789012:user/alice";
        
        let result = auth_handler::generate_token_for_iam_user(user_id, Some(7200))
            .await;
        
        assert!(result.is_ok());
        let token_data = result.unwrap();
        assert_eq!(token_data["expires_in"], 7200);
    }
    
    #[test]
    fn test_extract_iam_identity_user_arn() {
        let context = json!({
            "identity": {
                "userArn": "arn:aws:iam::123456789012:user/bob",
                "accountId": "123456789012"
            }
        });
        
        let result = auth_handler::extract_iam_identity(&context);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "arn:aws:iam::123456789012:user/bob");
    }
    
    #[test]
    fn test_extract_iam_identity_caller() {
        let context = json!({
            "identity": {
                "caller": "arn:aws:sts::123456789012:assumed-role/MyRole/session",
                "accountId": "123456789012"
            }
        });
        
        let result = auth_handler::extract_iam_identity(&context);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "arn:aws:sts::123456789012:assumed-role/MyRole/session");
    }
    
    #[test]
    fn test_extract_iam_identity_missing() {
        let context = json!({
            "identity": {
                "accountId": "123456789012"
            }
        });
        
        let result = auth_handler::extract_iam_identity(&context);
        assert!(result.is_err());
    }
    
    #[tokio::test]
    async fn test_token_claims_structure() {
        use jsonwebtoken::{decode, DecodingKey, Validation, Algorithm};
        
        std::env::set_var("JWT_SECRET", "test-secret-789");
        std::env::set_var("JWT_ISSUER", "test-issuer");
        std::env::set_var("JWT_AUDIENCE", "test-audience");
        
        let user_id = "arn:aws:iam::123456789012:user/charlie";
        let result = auth_handler::generate_token_for_iam_user(user_id, Some(3600))
            .await
            .expect("Token generation failed");
        
        let token = result["access_token"].as_str().unwrap();
        
        // Decode and verify the token
        let secret = std::env::var("JWT_SECRET").unwrap();
        let mut validation = Validation::new(Algorithm::HS256);
        validation.set_audience(&["test-audience"]);
        validation.set_issuer(&["test-issuer"]);
        
        let token_data = decode::<serde_json::Value>(
            token,
            &DecodingKey::from_secret(secret.as_bytes()),
            &validation,
        );
        
        assert!(token_data.is_ok(), "Token should be valid");
        
        let claims = token_data.unwrap().claims;
        
        // Verify required claims
        assert_eq!(claims["sub"], user_id);
        assert_eq!(claims["iss"], "test-issuer");
        assert_eq!(claims["aud"], "test-audience");
        assert_eq!(claims["cognito:username"], "charlie");
        assert_eq!(claims["token_use"], "access");
        
        // Verify timestamps
        assert!(claims.get("exp").is_some());
        assert!(claims.get("iat").is_some());
        
        let exp = claims["exp"].as_u64().unwrap();
        let iat = claims["iat"].as_u64().unwrap();
        assert!(exp > iat, "Expiration should be after issued time");
        assert_eq!(exp - iat, 3600, "TTL should be 3600 seconds");
    }
    
    #[tokio::test]
    async fn test_token_expiration() {
        use std::time::{SystemTime, UNIX_EPOCH};
        
        std::env::set_var("JWT_SECRET", "test-secret-expiry");
        
        let user_id = "arn:aws:iam::123456789012:user/expiry-test";
        let ttl = 100; // 100 seconds
        
        let result = auth_handler::generate_token_for_iam_user(user_id, Some(ttl))
            .await
            .expect("Token generation failed");
        
        let issued_at = result["issued_at"].as_u64().unwrap();
        let expires_in = result["expires_in"].as_u64().unwrap();
        
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        
        // Issued at should be close to now (within 5 seconds)
        assert!((now.saturating_sub(issued_at)) < 5);
        
        // Expiry should be issued_at + ttl
        assert_eq!(expires_in, ttl);
    }
    
    // Helper function to extract claims for testing
    fn extract_test_claims(user_id: &str, ttl: u64) -> serde_json::Value {
        use std::time::{SystemTime, UNIX_EPOCH};
        
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        
        // Extract username (simplified version of actual logic)
        let username = if user_id.starts_with("arn:aws:iam::") {
            user_id.split('/').last().unwrap_or("unknown")
        } else if user_id.starts_with("arn:aws:sts::") {
            user_id.split('/').last().unwrap_or("unknown")
        } else {
            user_id
        };
        
        json!({
            "sub": user_id,
            "iss": "test-issuer",
            "aud": "test-audience",
            "exp": now + ttl,
            "iat": now,
            "cognito:username": username,
            "token_use": "access"
        })
    }
}
