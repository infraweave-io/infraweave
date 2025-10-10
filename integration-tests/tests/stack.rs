mod utils;
use utils::test_scaffold;

#[cfg(test)]
mod stack_tests {
    use super::*;
    use env_common::interface::GenericCloudHandler;
    use env_defs::{CloudProvider, TfLockProvider, TfRequiredProvider};
    use pretty_assertions::assert_eq;
    use serde_json::Value;
    use std::{collections::HashSet, env};

    #[tokio::test]
    async fn test_stack_publish_bucketcollection() {
        test_scaffold(|| async move {
            let lambda_endpoint_url = "http://127.0.0.1:8080";
            let handler = GenericCloudHandler::custom(lambda_endpoint_url).await;
            let current_dir = env::current_dir().expect("Failed to get current directory");

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
                    .join("stacks/bucketcollection-dev/")
                    .to_str()
                    .unwrap()
                    .to_string(),
                &"dev".to_string(),
                Some("0.1.2-dev+test.10"),
                None,
            )
            .await
            .unwrap();

            let track = "".to_string();

            let stacks = match handler.get_all_latest_stack(&track).await {
                Ok(stacks) => stacks,
                Err(_e) => {
                    let empty: Vec<env_defs::ModuleResp> = vec![];
                    empty
                }
            };

            assert_eq!(stacks.len(), 1);
            assert_eq!(stacks[0].module, "bucketcollection");
            assert_eq!(stacks[0].version, "0.1.2-dev+test.10");
            assert_eq!(stacks[0].track, "dev");

            let examples = stacks[0].clone().manifest.spec.examples.unwrap();
            assert_eq!(examples[0].name, "bucketcollection");
            assert_eq!(
                examples[0]
                    .variables
                    .get("bucket1a")
                    .unwrap()
                    .get("bucketName")
                    .unwrap(),
                "bucket1a-name",
            );

            assert_eq!(stacks[0].tf_extra_environment_variables.len(), 0);

            assert_eq!(stacks[0].tf_variables.len(), 3);
            assert_eq!(
                stacks[0]
                    .tf_variables
                    .iter()
                    .map(|v| v.name.as_str())
                    .collect::<HashSet<_>>(),
                HashSet::from_iter(vec![
                    "bucket1a__bucket_name",
                    "bucket1a__enable_acl",
                    "bucket2__enable_acl",
                ])
            );

            assert_eq!(stacks[0].tf_outputs.len(), 6);
            assert_eq!(
                stacks[0]
                    .tf_outputs
                    .iter()
                    .map(|o| o.name.as_str())
                    .collect::<HashSet<_>>(),
                HashSet::from_iter(vec![
                    "bucket1a__bucket_arn",
                    "bucket1a__region",
                    "bucket1a__sse_algorithm",
                    "bucket2__bucket_arn",
                    "bucket2__region",
                    "bucket2__sse_algorithm"
                ])
            );
        })
        .await;
    }

    #[tokio::test]
    async fn test_stack_publish_bucketcollection_missing_region() {
        // should add variable checks as well
        test_scaffold(|| async move {
            let lambda_endpoint_url = "http://127.0.0.1:8080";
            let handler = GenericCloudHandler::custom(lambda_endpoint_url).await;
            let current_dir = env::current_dir().expect("Failed to get current directory");

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

            let result = env_common::publish_stack(
                &handler,
                &current_dir
                    .join("stacks/bucketcollection-missing-region/")
                    .to_str()
                    .unwrap()
                    .to_string(),
                &"dev".to_string(),
                Some("0.1.2-dev+test.10"),
                None,
            )
            .await;

            assert_eq!(result.is_err(), true);
        })
        .await;
    }

    #[tokio::test]
    async fn test_stack_publish_bucketcollection_invalid_variables() {
        test_scaffold(|| async move {
            let lambda_endpoint_url = "http://127.0.0.1:8080";
            let handler = GenericCloudHandler::custom(lambda_endpoint_url).await;
            let current_dir = env::current_dir().expect("Failed to get current directory");

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

            let result = env_common::publish_stack(
                &handler,
                &current_dir
                    .join("stacks/bucketcollection-invalid-variable/")
                    .to_str()
                    .unwrap()
                    .to_string(),
                &"dev".to_string(),
                Some("0.1.2-dev+test.10"),
                None,
            )
            .await;

            assert_eq!(result.is_err(), true);
        })
        .await;
    }

    #[tokio::test]
    async fn test_stack_publish_route53records() {
        test_scaffold(|| async move {
            let lambda_endpoint_url = "http://127.0.0.1:8080";
            let handler = GenericCloudHandler::custom(lambda_endpoint_url).await;
            let current_dir = env::current_dir().expect("Failed to get current directory");

            env_common::publish_provider(
                &handler,
                &current_dir
                    .join("providers/aws-5-us-east-1/")
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
                    .join("modules/route53record/")
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
                    .join("modules/route53record/")
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
                    .join("stacks/route53records/")
                    .to_str()
                    .unwrap()
                    .to_string(),
                &"dev".to_string(),
                Some("0.1.4-dev+test.10"),
                None,
            )
            .await
            .unwrap();

            let track = "".to_string();

            let stacks = match handler.get_all_latest_stack(&track).await {
                Ok(stacks) => stacks,
                Err(_e) => {
                    let empty: Vec<env_defs::ModuleResp> = vec![];
                    empty
                }
            };

            assert_eq!(stacks.len(), 1);
            assert_eq!(stacks[0].module, "route53records");
            assert_eq!(stacks[0].version, "0.1.4-dev+test.10");
            assert_eq!(stacks[0].track, "dev");

            let examples = stacks[0].clone().manifest.spec.examples;
            assert_eq!(examples.is_none(), true);

            assert_eq!(stacks[0].tf_variables.len(), 6);

            assert!(
                stacks[0]
                    .tf_variables
                    .iter()
                    .any(|v| v.name == "route1__domain_name"
                        && v._type == "string"
                        && v.default == Some(Value::String("example.com".to_string()))),
                "variable route1__domain_name is not as expected",
            );

            assert!(
                stacks[0]
                    .tf_variables
                    .iter()
                    .any(|v| v.name == "route1__records"
                        && v._type == "list(string)"
                        && v.default
                            == Some(serde_json::json!(["dev1.example.com", "dev2.example.com"]))),
                "variable route1__records is not as expected",
            );

            assert!(
                stacks[0]
                    .tf_variables
                    .iter()
                    .any(|v| v.name == "route1__ttl"
                        && v._type == "number"
                        && v.default
                            == Some(Value::Number(serde_json::Number::from_u128(300).unwrap()))),
                "variable route1__ttl is not as expected",
            );

            assert!(
                stacks[0]
                    .tf_variables
                    .iter()
                    .any(|v| v.name == "route2__domain_name"
                        && v._type == "string"
                        && v.default == Some(Value::String("example.com".to_string()))),
                "variable route2__domain_name is not as expected",
            );

            assert!(
                stacks[0]
                    .tf_variables
                    .iter()
                    .any(|v| v.name == "route2__records"
                        && v._type == "list(string)"
                        && v.default
                            == Some(serde_json::json!(["uat1.example.com", "uat2.example.com"]))),
                "variable route2__records is not as expected",
            );

            assert!(
                stacks[0]
                    .tf_variables
                    .iter()
                    .any(|v| v.name == "route2__ttl" && v._type == "number" && v.default == None),
                "variable route2__ttl is not as expected",
            );
        })
        .await;
    }

    #[tokio::test]
    async fn test_stack_publish_providermix() {
        test_scaffold(|| async move {
            let lambda_endpoint_url = "http://127.0.0.1:8080";
            let handler = GenericCloudHandler::custom(lambda_endpoint_url).await;
            let current_dir = env::current_dir().expect("Failed to get current directory");

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

            env_common::publish_provider(
                &handler,
                &current_dir
                    .join("providers/helm-3/")
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
                    .join("modules/nginx-ingress/")
                    .to_str()
                    .unwrap()
                    .to_string(),
                &"dev".to_string(),
                Some("0.5.5-dev+test.1"),
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
                Some("0.1.2-dev+test.10"),
                None,
            )
            .await
            .unwrap();

            env_common::publish_stack(
                &handler,
                &current_dir
                    .join("stacks/providermix/")
                    .to_str()
                    .unwrap()
                    .to_string(),
                &"dev".to_string(),
                Some("0.1.4-dev+test.10"),
                None,
            )
            .await
            .unwrap();

            let track = "".to_string();

            let stacks = match handler.get_all_latest_stack(&track).await {
                Ok(stacks) => stacks,
                Err(_e) => {
                    let empty: Vec<env_defs::ModuleResp> = vec![];
                    empty
                }
            };

            assert_eq!(stacks.len(), 1);
            assert_eq!(stacks[0].module, "providermix");
            assert_eq!(stacks[0].version, "0.1.4-dev+test.10");
            assert_eq!(stacks[0].track, "dev");

            let examples = stacks[0].clone().manifest.spec.examples;
            assert_eq!(examples.is_none(), true);

            println!("stack: {:?}", stacks[0]);
            assert_eq!(stacks[0].tf_variables.len(), 3);

            assert!(
                stacks[0]
                    .tf_required_providers
                    .iter()
                    .any(|p| p.name == "aws"
                        && p.source == "registry.opentofu.org/hashicorp/aws".to_string()),
                "aws provider missing from required providers"
            );
            assert!(
                stacks[0]
                    .tf_required_providers
                    .iter()
                    .any(|p| p.name == "helm"
                        && p.source == "registry.opentofu.org/hashicorp/helm".to_string()),
                "helm provider missing from required providers"
            );

            assert_eq!(stacks[0].tf_required_providers.len(), 2);

            assert!(
                stacks[0]
                    .tf_lock_providers
                    .iter()
                    .any(|p| p.source == "registry.opentofu.org/hashicorp/aws".to_string()),
                "aws provider missing from locked providers"
            );
            assert!(
                stacks[0]
                    .tf_lock_providers
                    .iter()
                    .any(|p| p.source == "registry.opentofu.org/hashicorp/helm".to_string()),
                "helm provider missing from locked providers"
            );

            assert_eq!(stacks[0].tf_lock_providers.len(), 2);
        })
        .await;
    }
}
