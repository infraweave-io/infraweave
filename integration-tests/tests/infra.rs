mod utils;
use utils::test_scaffold;

#[cfg(test)]
mod infra_tests {
    use super::*;
    use env_common::{interface::GenericCloudHandler, logic::run_claim};
    use env_defs::{CloudProvider, CloudProviderCommon, ExtraData};
    use pretty_assertions::assert_eq;
    use serde::Deserialize;
    use std::env;

    #[tokio::test]
    async fn test_infra_apply_s3bucket_dev() {
        test_scaffold(|| async move {
            let lambda_endpoint_url = "http://127.0.0.1:8080";
            let handler = GenericCloudHandler::custom(lambda_endpoint_url).await;
            let current_dir = env::current_dir().expect("Failed to get current directory");

            env_common::publish_provider(
                &handler,
                current_dir.join("providers/aws-5/").to_str().unwrap(),
                Some("0.1.2"),
            )
            .await
            .unwrap();

            env_common::publish_module(
                &handler,
                current_dir.join("modules/s3bucket-dev/").to_str().unwrap(),
                "dev",
                Some("0.1.2-dev+test.10"),
                None,
            )
            .await
            .unwrap();

            let claim_path = current_dir.join("claims/s3bucket-dev-claim.yaml");
            let claim_yaml_str =
                std::fs::read_to_string(claim_path).expect("Failed to read claim.yaml");
            let claims: Vec<serde_yaml::Value> =
                serde_yaml::Deserializer::from_str(&claim_yaml_str)
                    .map(|doc| serde_yaml::Value::deserialize(doc).unwrap_or("".into()))
                    .collect();

            let environment = "k8s-cluster-1/playground".to_string();
            let command = "apply".to_string();
            let flags = vec![];
            let (job_id, deployment_id) = match run_claim(
                &handler,
                &claims[0],
                &environment,
                &command,
                flags,
                ExtraData::None,
                "",
            )
            .await
            {
                Ok((job_id, deployment_id, _)) => (job_id, deployment_id),
                Err(e) => {
                    println!("Error: {:?}", e);
                    ("error".to_string(), "error".to_string())
                }
            };

            println!("Job ID: {}", job_id);
            println!("Deployment ID: {}", deployment_id);

            assert_eq!(job_id, "running-test-job-id");

            let (deployment, dependencies) = match handler
                .get_deployment_and_dependents(&deployment_id, &environment, false)
                .await
            {
                Ok((deployment, dependencies)) => (deployment, dependencies),
                Err(_e) => panic!("{:?}", "error"),
            };

            assert_eq!(deployment.is_some(), true);
            assert_eq!(dependencies.len(), 0);

            let deployment = deployment.unwrap();
            assert_eq!(deployment.deployment_id, "s3bucket/my-s3bucket2");
            assert_eq!(deployment.module, "s3bucket");
            assert_eq!(deployment.environment, "k8s-cluster-1/playground");
            assert_eq!(
                deployment.reference,
                "https://github.com/some-repo/some-path/claim.yaml"
            );
        })
        .await;
    }

    #[tokio::test]
    async fn test_infra_apply_s3bucket_dev_snake_case() {
        test_scaffold(|| async move {
            let lambda_endpoint_url = "http://127.0.0.1:8080";
            let handler = GenericCloudHandler::custom(lambda_endpoint_url).await;
            let current_dir = env::current_dir().expect("Failed to get current directory");

            env_common::publish_provider(
                &handler,
                current_dir.join("providers/aws-5/").to_str().unwrap(),
                Some("0.1.2"),
            )
            .await
            .unwrap();

            env_common::publish_module(
                &handler,
                current_dir.join("modules/s3bucket-dev/").to_str().unwrap(),
                "dev",
                Some("0.1.2-dev+test.10"),
                None,
            )
            .await
            .unwrap();

            let claim_path = current_dir.join("claims/s3bucket-dev-claim-snake_case.yaml");
            let claim_yaml_str =
                std::fs::read_to_string(claim_path).expect("Failed to read claim.yaml");
            let claims: Vec<serde_yaml::Value> =
                serde_yaml::Deserializer::from_str(&claim_yaml_str)
                    .map(|doc| serde_yaml::Value::deserialize(doc).unwrap_or("".into()))
                    .collect();

            let environment = "k8s-cluster-1/playground".to_string();
            let command = "apply".to_string();
            let flags = vec![];
            let res = run_claim(
                &handler,
                &claims[0],
                &environment,
                &command,
                flags,
                ExtraData::None,
                "",
            )
            .await;

            assert_eq!(res.is_ok(), false); // it should fail because the claim is using snake_case and it should be camelCase
        })
        .await;
    }

    #[tokio::test]
    async fn test_infra_apply_s3bucket_stable() {
        test_scaffold(|| async move {
            let lambda_endpoint_url = "http://127.0.0.1:8080";
            let handler = GenericCloudHandler::custom(lambda_endpoint_url).await;
            let current_dir = env::current_dir().expect("Failed to get current directory");

            env_common::publish_provider(
                &handler,
                current_dir.join("providers/aws-5/").to_str().unwrap(),
                Some("0.1.2"),
            )
            .await
            .unwrap();

            env_common::publish_module(
                &handler,
                current_dir
                    .join("modules/s3bucket-stable/")
                    .to_str()
                    .unwrap(),
                "stable",
                Some("0.1.2"),
                None,
            )
            .await
            .unwrap();

            let claim_path = current_dir.join("claims/s3bucket-stable-claim.yaml");
            let claim_yaml_str =
                std::fs::read_to_string(claim_path).expect("Failed to read claim.yaml");
            let claims: Vec<serde_yaml::Value> =
                serde_yaml::Deserializer::from_str(&claim_yaml_str)
                    .map(|doc| serde_yaml::Value::deserialize(doc).unwrap_or("".into()))
                    .collect();

            let environment = "k8s-cluster-1/playground".to_string();
            let command = "apply".to_string();
            let flags = vec![];
            let (job_id, deployment_id) = match run_claim(
                &handler,
                &claims[0],
                &environment,
                &command,
                flags,
                ExtraData::None,
                "reference-fallback",
            )
            .await
            {
                Ok((job_id, deployment_id, _)) => (job_id, deployment_id),
                Err(e) => {
                    println!("Error: {:?}", e);
                    ("error".to_string(), "error".to_string())
                }
            };

            println!("Job ID: {}", job_id);
            println!("Deployment ID: {}", deployment_id);

            assert_eq!(job_id, "running-test-job-id");

            let (deployment, dependencies) = match handler
                .get_deployment_and_dependents(&deployment_id, &environment, false)
                .await
            {
                Ok((deployment, dependencies)) => (deployment, dependencies),
                Err(_e) => panic!("{:?}", "error"),
            };

            assert_eq!(deployment.is_some(), true);
            assert_eq!(dependencies.len(), 0);

            let deployment = deployment.unwrap();
            assert_eq!(deployment.deployment_id, "s3bucket/my-s3bucket2");
            assert_eq!(deployment.module, "s3bucket");
            assert_eq!(deployment.environment, "k8s-cluster-1/playground");
            assert_eq!(deployment.reference, "reference-fallback");
        })
        .await;
    }

    #[tokio::test]
    async fn test_infra_nullable_variable_set_to_null() {
        test_scaffold(|| async move {
            let lambda_endpoint_url = "http://127.0.0.1:8080";
            let handler = GenericCloudHandler::custom(lambda_endpoint_url).await;
            let current_dir = env::current_dir().expect("Failed to get current directory");

            env_common::publish_provider(
                &handler,
                current_dir
                    .join("providers/aws-5/")
                    .to_str()
                    .unwrap(),
                Some("0.1.2"),
            )
            .await
            .unwrap();

            // Publish the test module
            env_common::publish_module(
                &handler,
                current_dir
                    .join("modules/test-nullable-with-default/")
                    .to_str()
                    .unwrap(),
                "dev",
                Some("0.1.0-dev"),
                None,
            )
            .await
            .unwrap();

            // Load the claim that sets myVar to null
            let claim_path = current_dir.join("claims/test-nullable-claim.yaml");
            let claim_yaml_str =
                std::fs::read_to_string(claim_path).expect("Failed to read claim.yaml");
            let claims: Vec<serde_yaml::Value> =
                serde_yaml::Deserializer::from_str(&claim_yaml_str)
                    .map(|doc| serde_yaml::Value::deserialize(doc).unwrap_or("".into()))
                    .collect();

            let environment = "k8s-cluster-1/playground".to_string();
            let command = "apply".to_string();
            let flags = vec![];

            // This should succeed if the validation correctly allows null for nullable variables
            let result = run_claim(
                &handler,
                &claims[0],
                &environment,
                &command,
                flags,
                ExtraData::None,
                "",
            )
            .await;

            match &result {
                Ok((job_id, deployment_id, _)) => {
                    println!("Success! Job ID: {}", job_id);
                    println!("Deployment ID: {}", deployment_id);
                    assert_eq!(*job_id, "running-test-job-id");

                    // Verify the deployment was created
                    let (deployment, _) = handler
                        .get_deployment_and_dependents(deployment_id, &environment, false)
                        .await
                        .expect("Failed to get deployment");

                    assert!(deployment.is_some(), "Deployment should exist");
                    let deployment = deployment.unwrap();

                    // Verify that myVar is set to null in the variables
                    let my_var = deployment.variables.get("my_var");
                    assert!(my_var.is_some(), "my_var should be present in variables");
                    assert_eq!(my_var.unwrap(), &serde_json::Value::Null, "my_var should be null");
                }
                Err(e) => {
                    // If this fails with the error about type mismatch, it confirms the bug
                    println!("Error occurred: {:?}", e);
                    let error_msg = format!("{:?}", e);
                    if error_msg.contains("Variable \"my_var\" is of type null but should be of type string") {
                        panic!("BUG CONFIRMED: Validation incorrectly rejects null for nullable variable with default value.\nError: {}", e);
                    } else {
                        panic!("Unexpected error: {}", e);
                    }
                }
            }
        })
        .await;
    }

    #[tokio::test]
    async fn test_deployment_in_progress_with_job_status_check() {
        test_scaffold(|| async move {
            let lambda_endpoint_url = "http://127.0.0.1:8080";
            let handler = GenericCloudHandler::custom(lambda_endpoint_url).await;
            let current_dir = env::current_dir().expect("Failed to get current directory");

            env_common::publish_provider(
                &handler,
                current_dir
                    .join("providers/aws-5/")
                    .to_str()
                    .unwrap(),
                Some("0.1.2"),
            )
            .await
            .unwrap();

            // Publish module
            env_common::publish_module(
                &handler,
                current_dir
                    .join("modules/s3bucket-dev/")
                    .to_str()
                    .unwrap(),
                "dev",
                Some("0.1.2-dev+test.10"),
                None,
            )
            .await
            .unwrap();

            let claim_path = current_dir.join("claims/s3bucket-dev-claim.yaml");
            let claim_yaml_str =
                std::fs::read_to_string(claim_path).expect("Failed to read claim.yaml");
            let claims: Vec<serde_yaml::Value> =
                serde_yaml::Deserializer::from_str(&claim_yaml_str)
                    .map(|doc| serde_yaml::Value::deserialize(doc).unwrap_or("".into()))
                    .collect();

            let environment = "k8s-cluster-1/playground".to_string();
            let command = "apply".to_string();
            let flags = vec![];

            // First deployment - should succeed
            let (job_id, deployment_id) = match run_claim(
                &handler,
                &claims[0],
                &environment,
                &command,
                flags.clone(),
                ExtraData::None,
                "",
            )
            .await
            {
                Ok((job_id, deployment_id, _)) => (job_id, deployment_id),
                Err(e) => {
                    println!("Error on first deployment: {:?}", e);
                    ("error".to_string(), "error".to_string())
                }
            };

            assert_eq!(job_id, "running-test-job-id");

            // Manually update the deployment to have a "initiated" status with a running job
            // The test lambda will return is_running=true for job IDs starting with "running-"
            let running_job_id = "running-test-job-123";
            let deployment = handler
                .get_deployment(&deployment_id, &environment, false)
                .await
                .unwrap()
                .unwrap();

            let mut updated_deployment = deployment.clone();
            updated_deployment.status = "initiated".to_string();
            updated_deployment.job_id = running_job_id.to_string();

            handler
                .set_deployment(&updated_deployment, false)
                .await
                .unwrap();

            // Second deployment attempt - should be blocked because job is running
            let result_blocked = run_claim(
                &handler,
                &claims[0],
                &environment,
                &command,
                flags.clone(),
                ExtraData::None,
                "",
            )
            .await;

            match result_blocked {
                Err(e) => {
                    let error_msg = format!("{:?}", e);
                    assert!(
                        error_msg.contains("A job for this deployment is already in progress"),
                        "Expected 'A job for this deployment is already in progress' error, got: {}",
                        error_msg
                    );
                    // Also verify the running job ID is included in the error
                    assert!(
                        error_msg.contains("running-test-job-123"),
                        "Expected error to include job ID 'running-test-job-123', got: {}",
                        error_msg
                    );
                }
                Ok(_) => {
                    panic!("Expected deployment to be blocked when job is running, but it succeeded");
                }
            }

            // Now update to a non-running job (simulating a stale deployment state)
            let non_running_job_id = "non-running-test-job-456";
            updated_deployment.job_id = non_running_job_id.to_string();

            handler
                .set_deployment(&updated_deployment, false)
                .await
                .unwrap();

            // Third deployment attempt - should succeed because job is not running
            let result_allowed = run_claim(
                &handler,
                &claims[0],
                &environment,
                &command,
                flags.clone(),
                ExtraData::None,
                "",
            )
            .await;

            match result_allowed {
                Ok((job_id, _deployment_id, _)) => {
                    assert_eq!(job_id, "running-test-job-id");
                    println!("Deployment allowed to proceed when job is not running (as expected)");
                }
                Err(e) => {
                    panic!("Expected deployment to succeed when job is not running, but got error: {:?}", e);
                }
            }
        })
        .await;
    }

    #[tokio::test]
    async fn test_module_deprecation_existing_deployment_can_modify() {
        test_scaffold(|| async move {
            let lambda_endpoint_url = "http://127.0.0.1:8080";
            let handler = GenericCloudHandler::custom(lambda_endpoint_url).await;
            let current_dir = env::current_dir().expect("Failed to get current directory");

            // Step 1: Publish provider
            env_common::publish_provider(
                &handler,
                current_dir
                    .join("providers/aws-5/")
                    .to_str()
                    .unwrap(),
                Some("0.1.2"),
            )
            .await
            .unwrap();

            // Step 2: Publish module version 0.1.2
            env_common::publish_module(
                &handler,
                current_dir
                    .join("modules/s3bucket-dev/")
                    .to_str()
                    .unwrap(),
                "dev",
                Some("0.1.2-dev+test.10"),
                None,
            )
            .await
            .unwrap();

            // Step 3: Create initial deployment using the module
            let claim_path = current_dir.join("claims/s3bucket-dev-claim.yaml");
            let claim_yaml_str =
                std::fs::read_to_string(claim_path).expect("Failed to read claim.yaml");
            let mut claims: Vec<serde_yaml::Value> =
                serde_yaml::Deserializer::from_str(&claim_yaml_str)
                    .map(|doc| serde_yaml::Value::deserialize(doc).unwrap_or("".into()))
                    .collect();

            // Modify the claim to explicitly use version 0.1.2
            if let Some(claim) = claims.get_mut(0) {
                if let Some(spec) = claim.get_mut("spec") {
                    if let Some(spec_map) = spec.as_mapping_mut() {
                        spec_map.insert(
                            serde_yaml::Value::String("moduleVersion".to_string()),
                            serde_yaml::Value::String("0.1.2-dev+test.10".to_string()),
                        );
                    }
                }
            }

            let environment = "k8s-cluster-1/playground".to_string();
            let command = "apply".to_string();
            let flags = vec![];

            let (job_id, deployment_id) = match run_claim(
                &handler,
                &claims[0],
                &environment,
                &command,
                flags.clone(),
                ExtraData::None,
                "",
            )
            .await
            {
                Ok((job_id, deployment_id, _)) => (job_id, deployment_id),
                Err(e) => panic!("Failed to create initial deployment: {:?}", e),
            };

            println!("Initial deployment - Job ID: {}", job_id);
            println!("Initial deployment - Deployment ID: {}", deployment_id);
            assert_eq!(job_id, "running-test-job-id");
            assert_eq!(deployment_id, "s3bucket/my-s3bucket2");

            // Mark the initial deployment as completed so we can modify it later
            let deployment = handler
                .get_deployment(&deployment_id, &environment, false)
                .await
                .unwrap()
                .unwrap();

            let mut updated_deployment = deployment.clone();
            updated_deployment.status = "ready".to_string();
            updated_deployment.job_id = "completed-job-id".to_string();

            handler
                .set_deployment(&updated_deployment, false)
                .await
                .unwrap();

            // Step 4: Publish a newer version 0.1.3 to ensure 0.1.2 is not the latest
            env_common::publish_module(
                &handler,
                current_dir
                    .join("modules/s3bucket-dev/")
                    .to_str()
                    .unwrap(),
                "dev",
                Some("0.1.3-dev+test.11"),
                None,
            )
            .await
            .unwrap();

            // Step 5: Deprecate version 0.1.2
            let deprecate_result = env_common::logic::deprecate_module(
                &handler,
                "s3bucket",
                "dev",
                "0.1.2-dev+test.10",
                Some("Test deprecation: Security vulnerability fixed in 0.1.3"),
            )
            .await;

            assert!(
                deprecate_result.is_ok(),
                "Failed to deprecate module: {:?}",
                deprecate_result.err()
            );
            println!("Successfully deprecated module version 0.1.2");

            // Step 6: Modify the existing deployment - this should succeed
            let modify_result = run_claim(
                &handler,
                &claims[0],
                &environment,
                &command,
                flags.clone(),
                ExtraData::None,
                "",
            )
            .await;

            match modify_result {
                Ok((job_id, deployment_id, _)) => {
                    println!("Modification allowed - Job ID: {}", job_id);
                    println!("Modification allowed - Deployment ID: {}", deployment_id);
                    assert_eq!(job_id, "running-test-job-id");
                    assert_eq!(deployment_id, "s3bucket/my-s3bucket2");
                }
                Err(e) => {
                    panic!(
                        "Expected existing deployment to be able to modify deprecated module, but got error: {:?}",
                        e
                    );
                }
            }
        })
        .await;
    }

    #[tokio::test]
    async fn test_module_deprecation_new_deployment_blocked() {
        test_scaffold(|| async move {
            let lambda_endpoint_url = "http://127.0.0.1:8080";
            let handler = GenericCloudHandler::custom(lambda_endpoint_url).await;
            let current_dir = env::current_dir().expect("Failed to get current directory");

            // Step 1: Publish provider
            env_common::publish_provider(
                &handler,
                current_dir
                    .join("providers/aws-5/")
                    .to_str()
                    .unwrap(),
                Some("0.1.2"),
            )
            .await
            .unwrap();

            // Step 2: Publish module version 0.1.4
            env_common::publish_module(
                &handler,
                current_dir
                    .join("modules/s3bucket-dev/")
                    .to_str()
                    .unwrap(),
                "dev",
                Some("0.1.4-dev+test.20"),
                None,
            )
            .await
            .unwrap();

            // Step 3: Publish a newer version 0.1.5 to ensure 0.1.4 is not the latest
            env_common::publish_module(
                &handler,
                current_dir
                    .join("modules/s3bucket-dev/")
                    .to_str()
                    .unwrap(),
                "dev",
                Some("0.1.5-dev+test.21"),
                None,
            )
            .await
            .unwrap();

            // Step 4: Deprecate version 0.1.4 BEFORE any deployment is created
            let deprecate_result = env_common::logic::deprecate_module(
                &handler,
                "s3bucket",
                "dev",
                "0.1.4-dev+test.20",
                Some("Critical bug found, use 0.1.5 instead"),
            )
            .await;

            assert!(
                deprecate_result.is_ok(),
                "Failed to deprecate module: {:?}",
                deprecate_result.err()
            );
            println!("Successfully deprecated module version 0.1.4");

            // Step 5: Try to create a NEW deployment using the deprecated version
            // We need a claim that uses version 0.1.4
            let claim_path = current_dir.join("claims/s3bucket-dev-claim.yaml");
            let claim_yaml_str =
                std::fs::read_to_string(claim_path).expect("Failed to read claim.yaml");

            // Parse and modify the claim to use version 0.1.4
            let mut claims: Vec<serde_yaml::Value> =
                serde_yaml::Deserializer::from_str(&claim_yaml_str)
                    .map(|doc| serde_yaml::Value::deserialize(doc).unwrap_or("".into()))
                    .collect();

            // Modify the claim to use deprecated version 0.1.4
            if let Some(claim) = claims.get_mut(0) {
                if let Some(spec) = claim.get_mut("spec") {
                    if let Some(spec_map) = spec.as_mapping_mut() {
                        spec_map.insert(
                            serde_yaml::Value::String("moduleVersion".to_string()),
                            serde_yaml::Value::String("0.1.4-dev+test.20".to_string()),
                        );
                    }
                }
            }

            let environment = "k8s-cluster-1/playground-new".to_string();
            let command = "apply".to_string();
            let flags = vec![];

            let result = run_claim(
                &handler,
                &claims[0],
                &environment,
                &command,
                flags,
                ExtraData::None,
                "",
            )
            .await;

            // Step 6: Verify that the deployment was blocked
            match result {
                Err(e) => {
                    let error_msg = format!("{:?}", e);
                    println!("Deployment blocked (as expected): {}", error_msg);

                    // Verify the error message mentions deprecation
                    assert!(
                        error_msg.contains("deprecated") || error_msg.contains("0.1.4"),
                        "Expected error to mention deprecation, got: {}",
                        error_msg
                    );

                    // Verify the deprecation message is included
                    assert!(
                        error_msg.contains("Critical bug found") || error_msg.contains("use 0.1.5 instead"),
                        "Expected error to include deprecation message, got: {}",
                        error_msg
                    );
                }
                Ok((job_id, deployment_id, _)) => {
                    panic!(
                        "Expected new deployment to be blocked when using deprecated module, but it succeeded with job_id: {}, deployment_id: {}",
                        job_id, deployment_id
                    );
                }
            }
        })
        .await;
    }
}
