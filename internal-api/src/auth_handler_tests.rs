#[cfg(test)]
mod auth_handler_tests {
    use internal_api::auth_handler;

    #[test]
    fn test_allowed_projects_claim_key_default() {
        std::env::remove_var("AUTH_ALLOWED_PROJECTS_CLAIM");
        assert_eq!(
            auth_handler::allowed_projects_claim_key(),
            "custom:allowed_projects"
        );
    }

    #[test]
    fn test_allowed_projects_claim_key_custom() {
        std::env::set_var("AUTH_ALLOWED_PROJECTS_CLAIM", "groups");
        assert_eq!(auth_handler::allowed_projects_claim_key(), "groups");
        std::env::remove_var("AUTH_ALLOWED_PROJECTS_CLAIM");
    }

    #[test]
    fn test_publish_permissions_claim_key_default() {
        std::env::remove_var("AUTH_PUBLISH_PERMISSIONS_CLAIM");
        assert_eq!(
            auth_handler::publish_permissions_claim_key(),
            "custom:publish_permissions"
        );
    }

    #[test]
    fn test_publish_permissions_claim_key_custom() {
        std::env::set_var("AUTH_PUBLISH_PERMISSIONS_CLAIM", "roles");
        assert_eq!(auth_handler::publish_permissions_claim_key(), "roles");
        std::env::remove_var("AUTH_PUBLISH_PERMISSIONS_CLAIM");
    }

    #[test]
    fn test_username_claim_keys_default() {
        std::env::remove_var("AUTH_USERNAME_CLAIMS");
        let keys = auth_handler::username_claim_keys();
        assert_eq!(keys, vec!["sub", "cognito:username", "email"]);
    }

    #[test]
    fn test_username_claim_keys_custom() {
        std::env::set_var("AUTH_USERNAME_CLAIMS", "sub,oid,email,upn");
        let keys = auth_handler::username_claim_keys();
        assert_eq!(keys, vec!["sub", "oid", "email", "upn"]);
        std::env::remove_var("AUTH_USERNAME_CLAIMS");
    }
}

#[cfg(all(test, feature = "aws"))]
mod aws_auth_tests {
    use internal_api::auth_handler;
    use serde_json::json;

    #[test]
    fn test_extract_username_from_iam_user_arn() {
        let arn = "arn:aws:iam::123456789012:user/alice";
        assert_eq!(auth_handler::extract_username_from_arn(arn), "alice");
    }

    #[test]
    fn test_extract_username_from_assumed_role() {
        let arn = "arn:aws:sts::123456789012:assumed-role/MyRole/session-123";
        assert_eq!(
            auth_handler::extract_username_from_arn(arn),
            "session-123"
        );
    }

    #[test]
    fn test_extract_iam_identity_user_arn() {
        let context = json!({
            "authorizer": {
                "iam": {
                    "userArn": "arn:aws:iam::123456789012:user/bob"
                }
            }
        });

        let result = auth_handler::extract_iam_identity(&context);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "arn:aws:iam::123456789012:user/bob");
    }

    #[test]
    fn test_extract_iam_identity_legacy_caller() {
        let context = json!({
            "identity": {
                "caller": "arn:aws:sts::123456789012:assumed-role/MyRole/session"
            }
        });

        let result = auth_handler::extract_iam_identity(&context);
        assert!(result.is_ok());
        assert_eq!(
            result.unwrap(),
            "arn:aws:sts::123456789012:assumed-role/MyRole/session"
        );
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
}
