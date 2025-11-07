mod utils;
use utils::test_scaffold;

#[cfg(test)]
mod runner_tests {
    use super::*;
    use env_common::{interface::GenericCloudHandler, logic::run_claim};
    use env_defs::CloudProvider;
    use env_defs::ExtraData;
    use env_defs::OciArtifactSet;
    use pretty_assertions::assert_eq;
    use serde::Deserialize;
    use serde_json::json;
    use std::env;
    use terraform_runner::run_terraform_runner;

    #[tokio::test]
    #[ignore = "OCI signing and attestation is problematic in test"]
    async fn test_runner_oci() {
        test_scaffold(|| async move {
            let lambda_endpoint_url = "http://127.0.0.1:8080";
            let handler = GenericCloudHandler::custom(lambda_endpoint_url).await;
            let current_dir = env::current_dir().expect("Failed to get current directory");
            env_common::publish_module(
                &handler,
                &current_dir
                    .join("modules/s3bucket-oci/")
                    .to_str()
                    .unwrap()
                    .to_string(),
                &"dev".to_string(),
                None,
        Some(OciArtifactSet {
                    oci_artifact_path: "oci-artifacts/".to_string(),
                    tag_main: "s3bucket-0.0.36-dev-test.198".to_string(),
                    tag_attestation: Some(
                        "sha256-1559cd5049bed772aa9a780a607e019d9a7e8a738787a23556cfdf7c41030f6e.att".to_string(),
                    ),
                    tag_signature: Some(
                        "sha256-1559cd5049bed772aa9a780a607e019d9a7e8a738787a23556cfdf7c41030f6e.sig".to_string(),
                    ),
                    digest: "sha256:1559cd5049bed772aa9a780a607e019d9a7e8a738787a23556cfdf7c41030f6e".to_string(),
                }),
            )
            .await
            .unwrap();

            // Upload artifacts that would have been uploaded to the OCI registry
            let files = [
                "oci-artifacts/s3bucket-0.0.36-dev-test.198.tar.gz",
                "oci-artifacts/sha256-1559cd5049bed772aa9a780a607e019d9a7e8a738787a23556cfdf7c41030f6e.att.tar.gz",
                "oci-artifacts/sha256-1559cd5049bed772aa9a780a607e019d9a7e8a738787a23556cfdf7c41030f6e.sig.tar.gz",
            ];
            for file in files.iter() {
                utils::upload_file(
                    &handler,
                    &file.to_string(),
                    &current_dir.join(file).to_str().unwrap().to_string(),
                )
                .await
                .unwrap();
            }

            let claim_path = current_dir.join("claims/s3bucket-oci-claim.yaml");
            let claim_yaml_str =
                std::fs::read_to_string(claim_path).expect("Failed to read claim.yaml");
            let claims: Vec<serde_yaml::Value> =
                serde_yaml::Deserializer::from_str(&claim_yaml_str)
                    .map(|doc| serde_yaml::Value::deserialize(doc).unwrap_or("".into()))
                    .collect();

            let environment = "k8s-cluster-1/playground".to_string();
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
            env::set_var("TF_BUCKET", "dummy-tf-bucket");
            env::set_var("REGION", "dummy-region");
            env::set_var("OCI_ARTIFACT_MODE", "true");

            // Set cloud provider specific environment variables
            match handler.get_cloud_provider() {
                "aws" => {
                    env::set_var("TF_DYNAMODB_TABLE", "dummy-dynamodb-table");
                }
                "azure" => {
                    env::set_var("CONTAINER_GROUP_NAME", "running-test-job-id");
                    env::set_var("ACCOUNT_ID", "dummy-account-id");
                    env::set_var("STORAGE_ACCOUNT", "dummy-storage-account");
                    env::set_var("RESOURCE_GROUP_NAME", "dummy-resource-group");
                }
                _ => panic!("Unsupported cloud provider"),
            }

            env::set_current_dir(env::temp_dir()).expect("Failed to set current directory");

            let default_policy_content = r#"package verification

# Default deny all attestations
default allow = false

# Allow attestations that are valid SLSA provenance with expected repo and branch
allow if {
    is_slsa_provenance
    is_expected_repository
    is_expected_branch
}

# Validate predicate type is SLSA provenance
is_slsa_provenance if {
    contains(input.attestation.predicateType, "slsa.dev/provenance")
}

# Validate repository matches expected value (with wildcard support)
is_expected_repository if {
    config_uri := input.attestation.predicate.invocation.configSource.uri
    # Extract repo part from URI (between github.com/ and @)
    github_prefix := "git+https://github.com/"
    startswith(config_uri, github_prefix)
    uri_after_github := substring(config_uri, count(github_prefix), -1)
    repo_part := split(uri_after_github, "@")[0]

    startswith(repo_part, input.config.expected_repository_prefix)
}

# Validate branch matches expected value
is_expected_branch if {
    config_uri := input.attestation.predicate.invocation.configSource.uri
    expected_branch_suffix := sprintf("@refs/heads/%s", [input.config.expected_branch])
    endswith(config_uri, expected_branch_suffix)
}"#;
            let policy = json!({
                "expected_repository_prefix": "InfiniteTabsOrg/module-",
                "expected_branch": "main",
                "policy_content": default_policy_content
            });

            env::set_var("ATTESTATION_POLICY", serde_json::to_string(&policy).unwrap());

            run_terraform_runner(&handler).await.unwrap();

            match handler
                .get_deployment(&deployment_id, &environment, false)
                .await
            {
                Ok(deployment) => {
                    assert_eq!(deployment.is_some(), true);
                    assert_eq!(deployment.unwrap().status, "successful"); // This is set as last step in the runner
                }
                Err(_e) => Err("Failed to get deployment").unwrap(),
            };

            // TODO: Mock the commands and verify that all expected commands were run
        })
        .await;
    }
}
