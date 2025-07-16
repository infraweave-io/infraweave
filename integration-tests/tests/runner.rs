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

    #[tokio::test]
    async fn test_runner() {
        test_scaffold(|| async move {
            let lambda_endpoint_url = "http://127.0.0.1:8080";
            let handler = GenericCloudHandler::custom(lambda_endpoint_url).await;
            let current_dir = env::current_dir().expect("Failed to get current directory");
            env_common::publish_module(
                &handler,
                &current_dir
                    .join("modules/s3bucket-dev/")
                    .to_str()
                    .unwrap()
                    .to_string(),
                &"dev".to_string(),
                Some("0.1.2-dev+test.10"),
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

            assert_eq!(job_id, "test-job-id");

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

            // Set cloud provider specific environment variables
            match handler.get_cloud_provider() {
                "aws" => {
                    env::set_var("TF_DYNAMODB_TABLE", "dummy-dynamodb-table");
                }
                "azure" => {
                    env::set_var("CONTAINER_GROUP_NAME", "test-job-id");
                    env::set_var("ACCOUNT_ID", "dummy-account-id");
                    env::set_var("STORAGE_ACCOUNT", "dummy-storage-account");
                    env::set_var("RESOURCE_GROUP_NAME", "dummy-resource-group");
                }
                _ => panic!("Unsupported cloud provider"),
            }

            env::set_current_dir(env::temp_dir()).expect("Failed to set current directory");

            run_terraform_runner(&handler).await.unwrap();
        })
        .await;
    }
}
