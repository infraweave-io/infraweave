mod utils;
use utils::test_scaffold;

#[cfg(test)]
mod stack_tests {
    use super::*;
    use env_common::{download_to_vec_from_modules, interface::GenericCloudHandler};
    use env_defs::CloudProvider;
    use env_utils::read_tf_from_zip;
    use hcl::Expression;
    use pretty_assertions::assert_eq;
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

            assert_eq!(
                stacks[0]
                    .tf_providers
                    .iter()
                    .flat_map(|p| p.tf_extra_environment_variables.iter())
                    .count(),
                15
            );
            assert_eq!(stacks[0].tf_extra_environment_variables.len(), 0);

            assert_eq!(
                stacks[0]
                    .tf_variables
                    .iter()
                    .map(|v| v.name.as_str())
                    .collect::<HashSet<&str>>(),
                HashSet::from_iter(vec![
                    "bucket1a__bucket_name",
                    "bucket1a__enable_acl",
                    "bucket2__enable_acl",
                ])
            );

            assert_eq!(
                stacks[0]
                    .tf_outputs
                    .iter()
                    .map(|o| o.name.as_str())
                    .collect::<HashSet<&str>>(),
                HashSet::from_iter(vec![
                    "bucket1a__bucket_arn",
                    "bucket1a__region",
                    "bucket1a__sse_algorithm",
                    "bucket2__bucket_arn",
                    "bucket2__region",
                    "bucket2__sse_algorithm",
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
    async fn test_stack_publish_route53records_with_exposed_provider_variables() {
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
                    .join("stacks/route53records-input-all/")
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

            assert_eq!(
                stacks[0]
                    .tf_providers
                    .iter()
                    .flat_map(|provider| provider.tf_variables.iter())
                    .count(),
                1,
                "Incorrect number of provider variables"
            );

            assert_eq!(
                stacks[0].tf_variables.len(),
                6,
                "Incorrect number of module variables"
            );

            if let Some(route1_records) = stacks[0]
                .tf_variables
                .iter()
                .find(|v| v.name == "route1__records")
            {
                assert_eq!(route1_records._type, "list(string)");
                assert_eq!(
                    route1_records.default,
                    Some(serde_json::json!(["dev1.example.com", "dev2.example.com"]))
                );
            } else {
                assert!(false, "route1__records is missing")
            }

            if let Some(route1_ttl) = stacks[0]
                .tf_variables
                .iter()
                .find(|v| v.name == "route1__ttl")
            {
                assert_eq!(route1_ttl._type, "number");
                assert_eq!(route1_ttl.default, Some(serde_json::json!(300))); // Default value in variables.tf is null, but 300 is set in claim
            } else {
                assert!(false, "route1__ttl is missing");
            }

            if let Some(route2_records) = stacks[0]
                .tf_variables
                .iter()
                .find(|v| v.name == "route2__records")
            {
                assert_eq!(route2_records._type, "list(string)");
                assert_eq!(
                    route2_records.default,
                    Some(serde_json::json!(["uat1.example.com", "uat2.example.com"]))
                );
            } else {
                assert!(false, "route1__records is missing")
            }

            if let Some(route2_ttl) = stacks[0]
                .tf_variables
                .iter()
                .find(|v| v.name == "route2__ttl")
            {
                assert_eq!(route2_ttl._type, "number");
                assert_eq!(route2_ttl.default, None);
            } else {
                assert!(false, "route1__ttl is missing");
            }
        })
        .await;
    }

    #[tokio::test]
    async fn test_stack_publish_route53records_no_exposed_provider_variables() {
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
                    .join("stacks/route53records-input-none/")
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

            assert_eq!(
                stacks[0]
                    .tf_providers
                    .iter()
                    .flat_map(|provider| provider.tf_variables.iter())
                    .count(),
                0,
                "Incorrect number of provider variables"
            );

            assert_eq!(
                stacks[0].tf_variables.len(),
                6,
                "Incorrect number of module variables"
            );

            if let Some(route1_records) = stacks[0]
                .tf_variables
                .iter()
                .find(|v| v.name == "route1__records")
            {
                assert_eq!(route1_records._type, "list(string)");
                assert_eq!(
                    route1_records.default,
                    Some(serde_json::json!(["dev1.example.com", "dev2.example.com"]))
                );
            } else {
                assert!(false, "route1__records is missing")
            }

            if let Some(route1_ttl) = stacks[0]
                .tf_variables
                .iter()
                .find(|v| v.name == "route1__ttl")
            {
                assert_eq!(route1_ttl._type, "number");
                assert_eq!(route1_ttl.default, Some(serde_json::json!(300))); // Default value in variables.tf is null, but 300 is set in claim
            } else {
                assert!(false, "route1__ttl is missing");
            }

            if let Some(route2_records) = stacks[0]
                .tf_variables
                .iter()
                .find(|v| v.name == "route2__records")
            {
                assert_eq!(route2_records._type, "list(string)");
                assert_eq!(
                    route2_records.default,
                    Some(serde_json::json!(["uat1.example.com", "uat2.example.com"]))
                );
            } else {
                assert!(false, "route1__records is missing")
            }

            if let Some(route2_ttl) = stacks[0]
                .tf_variables
                .iter()
                .find(|v| v.name == "route2__ttl")
            {
                assert_eq!(route2_ttl._type, "number");
                assert_eq!(route2_ttl.default, None);
            } else {
                assert!(false, "route1__ttl is missing");
            }
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

            assert_eq!(stacks[0].tf_variables.len(), 3);
            assert_eq!(
                stacks[0]
                    .tf_providers
                    .iter()
                    .flat_map(|p| p.tf_variables.iter())
                    .count(),
                4
            );

            assert_eq!(
                true,
                stacks[0]
                    .tf_required_providers
                    .iter()
                    .any(|rp| rp.name == "aws" && rp.source.ends_with("hashicorp/aws"))
            );
            assert_eq!(
                true,
                stacks[0]
                    .tf_required_providers
                    .iter()
                    .any(|rp| rp.name == "helm" && rp.source.ends_with("hashicorp/helm"))
            );
            assert_eq!(stacks[0].tf_required_providers.len(), 2);

            assert_eq!(
                true,
                stacks[0]
                    .tf_lock_providers
                    .iter()
                    .any(|rp| rp.source.ends_with("hashicorp/aws"))
            );
            assert_eq!(
                true,
                stacks[0]
                    .tf_lock_providers
                    .iter()
                    .any(|rp| rp.source.ends_with("hashicorp/helm"))
            );
            assert_eq!(stacks[0].tf_lock_providers.len(), 2);
        })
        .await;
    }

    #[tokio::test]
    async fn test_stack_publish_static_website() {
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
                    .join("modules/route53alias/")
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
                    .join("modules/s3bucket-web/")
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
                    .join("stacks/static-website/")
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
            assert_eq!(stacks[0].module, "staticwebsite");
            assert_eq!(stacks[0].version, "0.1.4-dev+test.10");
            assert_eq!(stacks[0].track, "dev");

            let examples = stacks[0].clone().manifest.spec.examples;
            assert_eq!(examples.is_none(), true);

            assert_eq!(stacks[0].tf_variables.len(), 2);
            assert_eq!(
                stacks[0]
                    .tf_providers
                    .iter()
                    .flat_map(|p| p.tf_variables.iter())
                    .map(|v| &v.name)
                    .collect::<HashSet<&String>>()
                    .len(),
                1
            );

            assert_eq!(
                true,
                stacks[0]
                    .tf_required_providers
                    .iter()
                    .any(|p| p.name == "aws" && p.source.ends_with("hashicorp/aws"))
            );
            assert_eq!(stacks[0].tf_required_providers.len(), 1);

            assert_eq!(
                true,
                stacks[0]
                    .tf_lock_providers
                    .iter()
                    .any(|p| p.source.ends_with("hashicorp/aws"))
            );
            assert_eq!(stacks[0].tf_lock_providers.len(), 1);
        })
        .await;
    }

    #[tokio::test]
    async fn test_stack_publish_webapp_example() {
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

            env_common::publish_provider(
                &handler,
                &current_dir
                    .join("providers/kubernetes-2/")
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
                    .join("modules/eks/")
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
                    .join("modules/webapp/")
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
                    .join("modules/s3bucket-with-backup/")
                    .to_str()
                    .unwrap()
                    .to_string(),
                &"dev".to_string(),
                Some("0.5.5-dev+test.1"),
                None,
            )
            .await
            .unwrap();

            env_common::publish_stack(
                &handler,
                &current_dir
                    .join("stacks/webapp-example/")
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
            assert_eq!(stacks[0].module, "webappexample");
            assert_eq!(stacks[0].version, "0.1.4-dev+test.10");
            assert_eq!(stacks[0].track, "dev");

            let examples = stacks[0].clone().manifest.spec.examples;
            assert_eq!(examples.is_none(), true);

            assert_eq!(stacks[0].tf_variables.len(), 11);
            assert_eq!(
                stacks[0]
                    .tf_providers
                    .iter()
                    .flat_map(|p| p.tf_variables.iter())
                    .map(|v| &v.name)
                    .collect::<HashSet<&String>>()
                    .len(),
                3
            );

            assert_eq!(
                true,
                stacks[0]
                    .tf_required_providers
                    .iter()
                    .any(|p| p.name == "aws" && p.source.ends_with("hashicorp/aws"))
            );
            assert_eq!(stacks[0].tf_required_providers.len(), 3);

            assert_eq!(
                true,
                stacks[0]
                    .tf_lock_providers
                    .iter()
                    .any(|p| p.source.ends_with("hashicorp/aws"))
            );
            assert_eq!(stacks[0].tf_lock_providers.len(), 7);

            let zip_data = download_to_vec_from_modules(&handler, &stacks[0].s3_key).await;
            let tf_content = read_tf_from_zip(&zip_data).unwrap();
            let body = hcl::parse(&tf_content).unwrap();
            let module_webapp = body.blocks().find(|b| {
                b.identifier() == "module"
                    && b.labels()
                        .contains(&hcl::BlockLabel::String("webapp".to_string()))
            });
            assert_ne!(module_webapp, None, "Missing webapp module");
            let depends_on = module_webapp
                .unwrap()
                .body()
                .attributes()
                .find(|attr| attr.key() == "depends_on");
            assert_ne!(depends_on, None, "Missing depends_on attirbute");
            if let Expression::Array(arr) = depends_on.unwrap().expr() {
                assert!(
                    arr.iter()
                        .map(|e| e.to_string())
                        .any(|e| e == "module.nginxingress"),
                    "Missing depends_on"
                );
            } else {
                assert!(false, "depens_on isn't an array")
            };
        })
        .await;
    }

    #[tokio::test]
    async fn test_stack_publish_webapp_example_manual() {
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

            env_common::publish_provider(
                &handler,
                &current_dir
                    .join("providers/kubernetes-2/")
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
                    .join("modules/eks/")
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
                    .join("modules/webapp/")
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
                    .join("modules/s3bucket-with-backup/")
                    .to_str()
                    .unwrap()
                    .to_string(),
                &"dev".to_string(),
                Some("0.5.5-dev+test.1"),
                None,
            )
            .await
            .unwrap();

            env_common::publish_stack(
                &handler,
                &current_dir
                    .join("stacks/webapp-example-manual/")
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
            assert_eq!(stacks[0].module, "webappexamplemanual");
            assert_eq!(stacks[0].version, "0.1.4-dev+test.10");
            assert_eq!(stacks[0].track, "dev");

            let examples = stacks[0].clone().manifest.spec.examples;
            assert_eq!(examples.is_none(), true);

            assert_eq!(stacks[0].tf_variables.len(), 11);
            assert_eq!(
                stacks[0]
                    .tf_providers
                    .iter()
                    .flat_map(|p| p.tf_variables.iter())
                    .map(|v| &v.name)
                    .collect::<HashSet<&String>>()
                    .len(),
                3
            );

            assert_eq!(
                true,
                stacks[0]
                    .tf_required_providers
                    .iter()
                    .any(|p| p.name == "aws" && p.source.ends_with("hashicorp/aws"))
            );
            assert_eq!(stacks[0].tf_required_providers.len(), 3);

            //version.tf override: pin aws version
            assert_eq!(
                true,
                stacks[0]
                    .tf_lock_providers
                    .iter()
                    .any(|p| p.source.ends_with("hashicorp/aws") && p.version == "5.96.0"),
                "Version not as specified in overriden version.tf",
            );
            assert_eq!(stacks[0].tf_lock_providers.len(), 7);

            let zip_data = download_to_vec_from_modules(&handler, &stacks[0].s3_key).await;
            let tf_content = read_tf_from_zip(&zip_data).unwrap();
            let body = hcl::parse(&tf_content).unwrap();

            //main.tf override: depends_on
            let module_webapp = body.blocks().find(|b| {
                b.identifier() == "module"
                    && b.labels()
                        .contains(&hcl::BlockLabel::String("webapp".to_string()))
            });
            assert_ne!(module_webapp, None, "Missing webapp module");
            let depends_on = module_webapp
                .unwrap()
                .body()
                .attributes()
                .find(|attr| attr.key() == "depends_on");
            assert_ne!(depends_on, None, "Missing depends_on attirbute");
            if let Expression::Array(arr) = depends_on.unwrap().expr() {
                assert!(
                    arr.iter()
                        .map(|e| e.to_string())
                        .any(|e| e == "module.nginxingress"),
                    "nginxingress missing in depends_on"
                );
                assert!(
                    arr.iter().map(|e| e.to_string()).any(|e| e == "module.eks"),
                    "eks missing in depends_on"
                );
            } else {
                assert!(false, "depens_on isn't an array")
            };

            //variables.tf override
            let variable = body.blocks().find(|b| {
                b.identifier() == "variable"
                    && b.labels()
                        .contains(&hcl::BlockLabel::String("kubernetes_token".to_string()))
            });
            assert_ne!(variable, None, "Missing \"kubernetes_token\" variable");
            let default_attr = variable
                .unwrap()
                .body()
                .attributes()
                .find(|attr| attr.key() == "default");
            assert_ne!(
                default_attr, None,
                "Missing default attribute on variable \"kubernetes_token\""
            );
            assert_eq!(
                default_attr.unwrap().expr(),
                &hcl::Expression::String("ABC123".to_string()),
                "Variable \"kubernetes_token\" has incorrect default value"
            );

            //provider.tf override
            let provider = body.blocks().find(|b| {
                b.identifier() == "provider"
                    && b.labels()
                        .contains(&hcl::BlockLabel::String("aws".to_string()))
            });
            assert_ne!(provider, None, "Missing \"aws\" provider block");
            let fips_attr = provider
                .unwrap()
                .body()
                .attributes()
                .find(|attr| attr.key() == "use_fips_endpoint");
            assert_ne!(
                fips_attr, None,
                "\"use_fips_endpoint\" attribute missing on \"aws\" provider"
            );
            assert_eq!(
                fips_attr.unwrap().expr(),
                &hcl::Expression::Bool(true),
                "\"use_fips_endpoint\" is not true"
            );
        })
        .await;
    }

    #[tokio::test]
    async fn test_stack_multiline_policy_with_reference() {
        test_scaffold(|| async move {
            let lambda_endpoint_url = "http://127.0.0.1:8080";
            let handler = GenericCloudHandler::custom(lambda_endpoint_url).await;
            let current_dir = env::current_dir().expect("Failed to get current directory");

            // Publish provider
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

            // Publish S3Bucket module for s3bucket
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

            // Publish IAMRole module for iamrole
            env_common::publish_module(
                &handler,
                &current_dir
                    .join("modules/iam-role-with-policy/")
                    .to_str()
                    .unwrap()
                    .to_string(),
                &"dev".to_string(),
                Some("0.1.0-dev+test.1"),
                None,
            )
            .await
            .unwrap();

            // Publish the stack
            env_common::publish_stack(
                &handler,
                &current_dir
                    .join("stacks/bucket-with-policy-reference/")
                    .to_str()
                    .unwrap()
                    .to_string(),
                &"dev".to_string(),
                Some("0.1.0-dev+test.1"),
                None,
            )
            .await
            .unwrap();

            let track = "".to_string();
            let stacks = handler.get_all_latest_stack(&track).await.unwrap();

            assert_eq!(stacks.len(), 1);
            assert_eq!(stacks[0].module, "bucketwithiampolicy");

            // Download and parse the generated Terraform
            let zip_data = download_to_vec_from_modules(&handler, &stacks[0].s3_key).await;
            let tf_content = read_tf_from_zip(&zip_data).unwrap();

            println!("Generated Terraform:\n{}", tf_content);

            let body = hcl::parse(&tf_content).unwrap();

            // Find the iamrole module
            let module_iamrole = body.blocks().find(|b| {
                b.identifier() == "module"
                    && b.labels()
                        .contains(&hcl::BlockLabel::String("iamrole".to_string()))
            });
            assert_ne!(module_iamrole, None, "Missing iamrole module");

            // Find the inline_policy attribute
            let inline_policy_attr = module_iamrole
                .unwrap()
                .body()
                .attributes()
                .find(|attr| attr.key() == "inline_policy");
            assert_ne!(inline_policy_attr, None, "Missing inline_policy attribute");

            // Verify the policy is a heredoc (not a quoted string with escaped newlines)
            match inline_policy_attr.unwrap().expr() {
                Expression::TemplateExpr(template) => {
                    match template.as_ref() {
                        hcl::expr::TemplateExpr::Heredoc(heredoc) => {
                            // Verify the references to both buckets were substituted
                            assert!(
                                heredoc.template.contains("${module.s3bucket.bucket_arn}"),
                                "Policy should contain reference to s3bucket's ARN, got: {}",
                                heredoc.template
                            );
                            assert!(
                                heredoc.template.contains("${module.s3bucket2.bucket_arn}"),
                                "Policy should contain reference to s3bucket2's ARN, got: {}",
                                heredoc.template
                            );
                            // Verify JSON structure is preserved
                            assert!(
                                heredoc.template.contains("s3:ListBucket"),
                                "Policy should contain s3:ListBucket action"
                            );
                            assert!(
                                heredoc.template.contains("s3:GetObject"),
                                "Policy should contain s3:GetObject action"
                            );
                            // Verify it's actual JSON with newlines, not escaped
                            assert!(
                                heredoc.template.contains("\n"),
                                "Policy should contain actual newlines, not escaped ones"
                            );
                        }
                        hcl::expr::TemplateExpr::QuotedString(_) => {
                            panic!(
                                "Policy should use Heredoc for multiline content, not QuotedString"
                            );
                        }
                    }
                }
                _ => {
                    panic!("Policy should be a TemplateExpr");
                }
            }
        })
        .await;
    }

    #[tokio::test]
    async fn test_stack_publish_with_stack_variables() {
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

            let track = "".to_string();

            let stacks = match handler.get_all_latest_stack(&track).await {
                Ok(stacks) => stacks,
                Err(_e) => {
                    let empty: Vec<env_defs::ModuleResp> = vec![];
                    empty
                }
            };

            let stack = stacks
                .iter()
                .find(|s| s.module == "bucketcollectionstackvars")
                .expect("Stack not found");

            assert_eq!(stack.version, "0.1.0-dev+test.1");
            assert_eq!(stack.track, "dev");

            // Check that stack-level variable was created
            let stack_vars: Vec<&str> = stack
                .tf_variables
                .iter()
                .filter(|v| v.name.starts_with("stack__"))
                .map(|v| v.name.as_str())
                .collect();

            assert!(
                stack_vars.contains(&"stack__environment"),
                "Stack variable 'stack__environment' not found. Found variables: {:?}",
                stack.tf_variables.iter().map(|v| &v.name).collect::<Vec<_>>()
            );

            // Check that bucket variables don't exist because they were replaced
            // with references to the stack variable in the dependency map
            let has_bucket1a_var = stack
                .tf_variables
                .iter()
                .any(|v| v.name == "bucket1a__bucket_name");

            let has_bucket2_var = stack
                .tf_variables
                .iter()
                .any(|v| v.name == "bucket2__bucket_name");

            // These should be false because the variables are resolved via dependency map
            assert!(
                !has_bucket1a_var,
                "bucket1a__bucket_name should not be a top-level variable (resolved via dependency map)"
            );
            assert!(
                !has_bucket2_var,
                "bucket2__bucket_name should not be a top-level variable (resolved via dependency map)"
            );

            // Verify example structure includes stack variables
            let examples = stack.clone().manifest.spec.examples.unwrap();
            assert_eq!(examples[0].name, "bucketcollection-stack-vars-example");

            let stack_vars_in_example = examples[0].variables.get("stack");
            assert!(
                stack_vars_in_example.is_some(),
                "Example doesn't have 'stack' section in variables"
            );

            let environment_val = stack_vars_in_example
                .unwrap()
                .get("environment");
            assert!(
                environment_val.is_some(),
                "Example doesn't have 'environment' in stack variables"
            );
            assert_eq!(
                environment_val.unwrap().as_str().unwrap(),
                "production"
            );
        })
        .await;
    }
}
