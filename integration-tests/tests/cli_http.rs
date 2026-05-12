/// Integration tests for the CLI HTTP transport mode.
///
/// These tests start the internal-api HTTP server in-process **once** (backed
/// by testcontainers DynamoDB + MinIO), then all tests reuse the same server.
/// This validates the full round-trip from the CLI's HTTP transport helpers
/// through the internal-api HTTP endpoints and back.

#[cfg(test)]
#[allow(deprecated)] // set_var/remove_var: safe here because all writes happen inside OnceCell::get_or_init before any test runs.
mod cli_http_tests {
    use env_common::interface::initialize_project_id_and_region;
    use pretty_assertions::assert_eq;
    use tokio::sync::OnceCell;

    /// Shared infrastructure + server port, initialized exactly once.
    static SERVER: OnceCell<u16> = OnceCell::const_new();

    /// Start local infra and HTTP server once; return the port for all tests.
    async fn ensure_server() -> u16 {
        *SERVER
            .get_or_init(|| async {
                // Set CWD to workspace root so relative paths in bootstrap work.
                let workspace_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                    .parent()
                    .expect("integration-tests must be inside workspace");
                std::env::set_current_dir(workspace_root)
                    .expect("Failed to set CWD to workspace root");

                // Leak the guard so containers stay alive for the entire test run.
                let infra = internal_api::local_setup::start_local_infrastructure()
                    .await
                    .expect("Failed to start local infrastructure");
                Box::leak(Box::new(infra));

                initialize_project_id_and_region().await;

                let port = internal_api::local_setup::start_test_server();

                // Remove TEST_MODE so is_http_mode_enabled() returns true.
                std::env::remove_var("TEST_MODE");
                std::env::set_var(
                    "INFRAWEAVE_API_ENDPOINT",
                    format!("http://127.0.0.1:{}", port),
                );

                // Write a tokens.json with the "local" sentinel to bypass JWT validation.
                let tokens_path =
                    env_utils::config_path::get_token_path().expect("Failed to get token path");
                std::fs::create_dir_all(tokens_path.parent().unwrap()).ok();
                let tokens_json = serde_json::json!({
                    "access_token": "local",
                    "id_token": "local",
                    "refresh_token": null,
                    "expires_at": null,
                    "api_endpoint": format!("http://127.0.0.1:{}", port),
                    "region": null
                });
                std::fs::write(&tokens_path, tokens_json.to_string())
                    .expect("Failed to write tokens.json");

                // Give the server a moment to start accepting connections.
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;

                port
            })
            .await
    }

    // ── Module tests ────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_http_list_modules() {
        let _port = ensure_server().await;

        // The scaffold seeds s3bucket-simple on the "stable" track
        let modules = http_client::http_get_all_latest_modules("stable")
            .await
            .expect("http_get_all_latest_modules failed");

        assert!(!modules.is_empty(), "Expected at least one seeded module");

        let names: Vec<&str> = modules.iter().map(|m| m.module.as_str()).collect();
        assert!(
            names.iter().any(|n| n.contains("s3bucket")),
            "Expected seeded s3bucket module, got: {:?}",
            names
        );
    }

    #[tokio::test]
    async fn test_http_get_module_version() {
        let _port = ensure_server().await;

        let module = http_client::http_get_module_version("stable", "s3bucketsimple", "1.0.0")
            .await
            .expect("http_get_module_version failed");

        assert_eq!(module.module, "s3bucketsimple");
        assert_eq!(module.version, "1.0.0");
    }

    #[tokio::test]
    async fn test_http_get_module_version_not_found() {
        let _port = ensure_server().await;

        let result =
            http_client::http_get_module_version("stable", "nonexistent-module", "0.0.0").await;

        assert!(result.is_err(), "Expected error for nonexistent module");
    }

    #[tokio::test]
    async fn test_http_get_all_versions_for_module() {
        let _port = ensure_server().await;

        let versions = http_client::http_get_all_versions_for_module("stable", "s3bucketsimple")
            .await
            .expect("http_get_all_versions_for_module failed");

        assert!(!versions.is_empty(), "Expected at least one version");
        assert!(versions.iter().any(|v| v.version == "1.0.0"));
    }

    // ── Provider tests ──────────────────────────────────────────────────

    #[tokio::test]
    async fn test_http_list_providers() {
        let _port = ensure_server().await;

        // The scaffold seeds the aws-5 provider
        let providers = http_client::http_get_all_latest_providers()
            .await
            .expect("http_get_all_latest_providers failed");

        assert!(
            !providers.is_empty(),
            "Expected at least one seeded provider"
        );
    }

    // ── Project tests ───────────────────────────────────────────────────

    #[tokio::test]
    async fn test_http_list_projects() {
        let _port = ensure_server().await;

        // In local mode with no JWT claims, get_projects filters out all
        // projects (no allowed_projects claim). The test verifies the HTTP
        // round-trip succeeds without error.
        let result = http_client::http_get_all_projects().await;
        assert!(
            result.is_ok(),
            "http_get_all_projects should succeed, got: {:?}",
            result.err()
        );
    }

    // ── Deployment tests (protected routes) ─────────────────────────────

    #[tokio::test]
    async fn test_http_list_deployments_empty() {
        let _port = ensure_server().await;

        let region = std::env::var("AWS_REGION").unwrap_or_else(|_| "us-west-2".to_string());
        // No deployments have been created, so the list should be empty
        let deployments = http_client::http_get_deployments("000000000000", &region)
            .await
            .expect("http_get_deployments failed");

        assert!(
            deployments.is_empty(),
            "Expected no deployments in fresh scaffold, got: {}",
            deployments.len()
        );
    }

    #[tokio::test]
    async fn test_http_describe_deployment_not_found() {
        let _port = ensure_server().await;

        let region = std::env::var("AWS_REGION").unwrap_or_else(|_| "us-west-2".to_string());
        let result = http_client::http_describe_deployment(
            "000000000000",
            &region,
            "dev/staging",
            "nonexistent/deployment",
        )
        .await;

        // Should return null for a non-existent deployment (not crash)
        assert!(
            result.is_ok(),
            "Expected graceful handling of missing deployment, got: {:?}",
            result.err()
        );
        let value = result.unwrap();
        assert!(
            value.is_null() || value == serde_json::json!(null),
            "Expected null for non-existent deployment"
        );
    }

    // ── Policy tests ────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_http_list_policies_empty() {
        let _port = ensure_server().await;

        let policies = http_client::http_get_policies("dev")
            .await
            .expect("http_get_policies failed");

        assert!(
            policies.is_empty(),
            "Expected no policies in fresh scaffold"
        );
    }
}
