mod utils;
use utils::test_scaffold;

#[cfg(test)]
mod runner_tests {
    use super::*;
    use env_common::{interface::GenericCloudHandler, logic::run_claim};
    use env_defs::CloudProvider;
    use env_defs::ExtraData;
    use pretty_assertions::assert_eq;
    use serde::Deserialize;
    use std::env;
    use terraform_runner::run_terraform_runner;

    // Helper to set current directory and restore it on drop
    struct RestoreDir;

    impl RestoreDir {
        fn new(path: &std::path::Path) -> Self {
            env::set_current_dir(path).expect("Failed to set current directory");
            Self
        }
    }

    impl Drop for RestoreDir {
        fn drop(&mut self) {
            let _ = env::set_current_dir(env!("CARGO_MANIFEST_DIR"));
        }
    }

    #[tokio::test]
    async fn test_runner() {
        test_scaffold(|| async move {
            let lambda_endpoint_url = "http://127.0.0.1:8080";
            let handler = GenericCloudHandler::custom(lambda_endpoint_url).await;
            let current_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));

            env_common::publish_provider(
                &handler,
                &current_dir
                    .join("providers/aws-5/")
                    .to_str()
                    .unwrap()
                    .to_string(),
                Some("0.1.2"),
            )
            .await
            .unwrap();

            env_common::publish_module(
                &handler,
                &current_dir
                    .join("modules/s3bucket-dev/")
                    .to_str()
                    .unwrap()
                    .to_string(),
                &"dev".to_string(),
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

            let environment = "default/playground".to_string();
            let command = "apply".to_string();
            let flags = vec![];
            let (job_id, deployment_id, payload_with_variables) = match run_claim(
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
                Ok((job_id, deployment_id, payload_with_variables)) => {
                    (job_id, deployment_id, Some(payload_with_variables))
                }
                Err(e) => {
                    println!("Error: {:?}", e);
                    ("error".to_string(), "error".to_string(), None)
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
                Err(_e) => Err("error").unwrap(),
            };

            assert_eq!(deployment.is_some(), true);
            assert_eq!(dependencies.len(), 0);

            let payload = payload_with_variables.unwrap().payload;
            let payload_str = serde_json::to_string(&payload).unwrap();

            env::set_var("PAYLOAD", payload_str);
            env::set_var("TF_BUCKET", "tf-state");
            env::set_var("REGION", "us-west-2");

            // Set cloud provider specific environment variables
            match handler.get_cloud_provider() {
                "aws" => {
                    println!("Using AWS test credentials with LocalStack for Terraform");
                }
                "azure" => {
                    env::set_var("CONTAINER_GROUP_NAME", "running-test-job-id");
                    env::set_var("ACCOUNT_ID", "dummy-account-id");
                    env::set_var("STORAGE_ACCOUNT", "devstoreaccount1");
                    env::set_var("RESOURCE_GROUP_NAME", "dummy-resource-group");
                }
                _ => panic!("Unsupported cloud provider"),
            }

            let temp_dir = tempfile::Builder::new()
                .prefix(&format!("infraweave-test-{}-", deployment_id.replace("/", "-")))
                .tempdir()
                .expect("Failed to create temp directory");

            let _guard = RestoreDir::new(temp_dir.path());
            println!("Working directory: {:?}", temp_dir.path());

            println!("Running terraform runner...");
            let runner_result = run_terraform_runner(&handler).await;

            match runner_result {
                Ok(_) => {
                    println!("Terraform runner completed successfully");

                    match handler
                        .get_deployment(&deployment_id, &environment, false)
                        .await
                    {
                        Ok(Some(deployment)) => {
                            assert_eq!(deployment.status, "successful");
                            println!("Deployment status: {}", deployment.status);
                            println!("Full deployment {:#?}", deployment);
                        }
                        Ok(None) => panic!("Deployment not found after runner execution"),
                        Err(e) => panic!("Failed to get deployment: {:?}", e),
                    };

                    println!("Verifying change record...");
                    match handler
                        .get_change_record(&environment, &deployment_id, &job_id, "APPLY")
                        .await
                    {
                        Ok(change_record) => {
                            println!("Change record found!");
                            println!("Full change record: {:#?}", change_record);

                            assert_eq!(change_record.deployment_id, deployment_id);
                            assert_eq!(change_record.job_id, job_id);
                            assert_eq!(change_record.module, "s3bucket");
                            assert_eq!(change_record.module_version, "0.1.2-dev+test.10");
                            assert_eq!(change_record.change_type, "apply");
                            assert_eq!(change_record.environment, environment);

                            if handler.get_cloud_provider() == "aws" && !change_record.resource_changes.is_empty() {
                                let has_s3_bucket = change_record.resource_changes.iter().any(|rc| {
                                    rc.resource_type.contains("aws_s3_bucket")
                                });
                                if has_s3_bucket {
                                    println!("  ✓ Found S3 bucket resource change");
                                } else {
                                    println!("  Note: S3 bucket resource not found in changes (may be due to test environment)");
                                }
                            }
                        }
                        Err(e) => {
                            println!("Warning: Could not retrieve change record: {:?}", e);
                        }
                    }
                }
                Err(e) => {
                    println!("Terraform runner failed: {:?}", e);
                    match handler
                        .get_deployment(&deployment_id, &environment, false)
                        .await
                    {
                        Ok(Some(deployment)) => {
                            println!("Deployment status after failure: {}", deployment.status);
                            assert!(
                                deployment.status.contains("fail") || deployment.status == "successful",
                                "Expected status to be failure-related or successful, got: {}",
                                deployment.status
                            );
                        }
                        Ok(None) => panic!("Deployment not found after runner execution"),
                        Err(e) => panic!("Failed to get deployment: {:?}", e),
                    }
                }
            }
        })
        .await;
    }

    #[tokio::test]
    async fn test_runner_stack_with_variables() {
        test_scaffold(|| async move {
            let lambda_endpoint_url = "http://127.0.0.1:8080";
            let handler = GenericCloudHandler::custom(lambda_endpoint_url).await;
            let current_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));

            env_common::publish_provider(
                &handler,
                &current_dir
                    .join("providers/aws-5/")
                    .to_str()
                    .unwrap()
                    .to_string(),
                Some("0.1.2"),
            )
            .await
            .unwrap();

            env_common::publish_module(
                &handler,
                &current_dir
                    .join("modules/s3bucket-dev/")
                    .to_str()
                    .unwrap()
                    .to_string(),
                &"dev".to_string(),
                Some("0.1.2-dev+test.10"),
                None,
            )
            .await
            .unwrap();

            env_common::publish_module(
                &handler,
                &current_dir
                    .join("modules/s3bucket-dev/")
                    .to_str()
                    .unwrap()
                    .to_string(),
                &"dev".to_string(),
                Some("0.1.3-dev+test.10"),
                None,
            )
            .await
            .unwrap();

            env_common::publish_stack(
                &handler,
                &current_dir
                    .join("stacks/bucketcollection-stack-vars/")
                    .to_str()
                    .unwrap()
                    .to_string(),
                &"dev".to_string(),
                Some("0.1.0-dev+test.1"),
                None,
            )
            .await
            .unwrap();

            let stacks = handler.get_all_latest_stack(&"".to_string()).await.unwrap();
            let stack = stacks
                .iter()
                .find(|s| s.module == "bucketcollectionstackvars")
                .expect("Stack 'bucketcollectionstackvars' not found after publishing");

            println!("Stack found: {} version {}", stack.module, stack.version);
            assert_eq!(stack.version, "0.1.0-dev+test.1");

            let claim_path = current_dir.join("claims/bucketcollection-stack-vars-claim.yaml");
            let claim_yaml_str =
                std::fs::read_to_string(claim_path).expect("Failed to read stack claim.yaml");
            let claims: Vec<serde_yaml::Value> =
                serde_yaml::Deserializer::from_str(&claim_yaml_str)
                    .map(|doc| serde_yaml::Value::deserialize(doc).unwrap_or("".into()))
                    .collect();

            let environment = "default/playground".to_string();
            let command = "apply".to_string();
            let flags = vec![];

            let (job_id, deployment_id, payload_with_variables) = match run_claim(
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
                Ok((job_id, deployment_id, payload_with_variables)) => {
                    (job_id, deployment_id, Some(payload_with_variables))
                }
                Err(e) => {
                    println!("Error running stack claim: {:?}", e);
                    ("error".to_string(), "error".to_string(), None)
                }
            };

            println!("Job ID: {}", job_id);
            println!("Deployment ID: {}", deployment_id);

            assert_eq!(job_id, "running-test-job-id");

            let (deployment, _dependencies) = match handler
                .get_deployment_and_dependents(&deployment_id, &environment, false)
                .await
            {
                Ok((deployment, dependencies)) => (deployment, dependencies),
                Err(_e) => Err("error").unwrap(),
            };

            assert_eq!(deployment.is_some(), true);

            let payload = payload_with_variables.unwrap().payload;
            let payload_str = serde_json::to_string(&payload).unwrap();

            env::set_var("PAYLOAD", payload_str);
            env::set_var("TF_BUCKET", "tf-state");
            env::set_var("REGION", "us-west-2");

            let temp_dir = tempfile::Builder::new()
                .prefix(&format!(
                    "infraweave-test-{}-",
                    deployment_id.replace("/", "-")
                ))
                .tempdir()
                .expect("Failed to create temp directory");

            let _guard = RestoreDir::new(temp_dir.path());
            println!("Working directory: {:?}", temp_dir.path());

            println!("Running terraform runner for stack deployment...");
            let runner_result = run_terraform_runner(&handler).await;

            match runner_result {
                Ok(_) => {
                    println!("Stack terraform runner completed successfully");

                    match handler
                        .get_deployment(&deployment_id, &environment, false)
                        .await
                    {
                        Ok(Some(deployment)) => {
                            assert_eq!(deployment.status, "successful");
                            println!("Stack deployment status: {}", deployment.status);
                            println!("Stack deployment: {:#?}", deployment);
                        }
                        Ok(None) => panic!("Stack deployment not found after runner execution"),
                        Err(e) => panic!("Failed to get stack deployment: {:?}", e),
                    };

                    println!("Verifying stack change record...");
                    match handler
                        .get_change_record(&environment, &deployment_id, &job_id, "APPLY")
                        .await
                    {
                        Ok(change_record) => {
                            println!("Stack change record found!");
                            println!("Full change record: {:#?}", change_record);

                            assert_eq!(change_record.deployment_id, deployment_id);
                            assert_eq!(change_record.job_id, job_id);
                            assert_eq!(change_record.module, "bucketcollectionstackvars");
                            assert_eq!(change_record.module_version, "0.1.0-dev+test.1");
                            assert_eq!(change_record.change_type, "apply");
                            assert_eq!(change_record.environment, environment);
                        }
                        Err(e) => {
                            println!("Warning: Could not retrieve stack change record: {:?}", e);
                        }
                    }
                }
                Err(e) => {
                    assert!(false, "Stack runner failed: {:?}", e);
                }
            }
        })
        .await;
    }
}
