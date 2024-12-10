mod utils;
use utils::test_scaffold;

#[cfg(test)]
mod infra_tests {
    use super::*;
    use env_common::{
        interface::CloudHandler,
        logic::{handler, run_claim},
    };
    use pretty_assertions::assert_eq;
    use serde::Deserialize;
    use std::env;

    #[tokio::test]
    async fn test_infra_apply_s3bucket() {
        test_scaffold(|| async move {
            let current_dir = env::current_dir().expect("Failed to get current directory");
            env_common::publish_module(
                &current_dir
                    .join("modules/s3bucket/")
                    .to_str()
                    .unwrap()
                    .to_string(),
                &"dev".to_string(),
                Some("0.1.2-dev+test.10"),
            )
            .await
            .unwrap();

            let claim_path = current_dir.join("claims/s3bucket-claim.yaml");
            let claim_yaml_str =
                std::fs::read_to_string(claim_path).expect("Failed to read claim.yaml");
            let claims: Vec<serde_yaml::Value> =
                serde_yaml::Deserializer::from_str(&claim_yaml_str)
                    .map(|doc| serde_yaml::Value::deserialize(doc).unwrap_or("".into()))
                    .collect();

            let environment = "dev".to_string();
            let command = "apply".to_string();
            let (job_id, deployment_id) = match run_claim(&claims[0], &environment, &command).await
            {
                Ok((job_id, deployment_id)) => (job_id, deployment_id),
                Err(e) => {
                    println!("Error: {:?}", e);
                    ("error".to_string(), "error".to_string())
                }
            };

            println!("Job ID: {}", job_id);
            println!("Deployment ID: {}", deployment_id);

            assert_eq!(job_id, "test-job-id");

            let (deployment, dependencies) = match handler()
                .get_deployment_and_dependents(&deployment_id, &environment, false)
                .await
            {
                Ok((deployment, dependencies)) => (deployment, dependencies),
                Err(_e) => Err("error").unwrap(),
            };

            assert_eq!(deployment.is_some(), true);
            assert_eq!(dependencies.len(), 0);

            let deployment = deployment.unwrap();
            assert_eq!(deployment.deployment_id, "s3bucket/my-s3bucket2");
            assert_eq!(deployment.module, "s3bucket");
            assert_eq!(deployment.environment, "dev");
        })
        .await;
    }
}
