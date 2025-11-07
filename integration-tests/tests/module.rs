mod utils;
use utils::test_scaffold;

#[cfg(test)]
mod module_tests {
    use super::*;
    use env_common::{download_to_vec_from_modules, interface::GenericCloudHandler};
    use env_defs::CloudProvider;
    use env_utils::{get_terraform_lockfile, get_terraform_tfvars};
    use pretty_assertions::assert_eq;
    use std::{collections::HashSet, env};

    #[tokio::test]
    async fn test_module_publish_s3bucket_invalid_provider() {
        test_scaffold(|| async move {
            let lambda_endpoint_url = "http://127.0.0.1:8080";
            let handler = GenericCloudHandler::custom(lambda_endpoint_url).await;
            let current_dir = env::current_dir().expect("Failed to get current directory");
            let publish_attempt = env_common::publish_module(
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
            .await;

            // Intentionally not publish provider first, so the module publish should fail due to missing provider

            let publish_successful = publish_attempt.is_ok();
            assert_eq!(publish_successful, false);
        })
        .await;
    }

    #[tokio::test]
    async fn test_module_publish_s3bucket() {
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

            let track = "".to_string();

            let modules = match handler.get_all_latest_module(&track).await {
                Ok(modules) => modules,
                Err(_e) => {
                    let empty: Vec<env_defs::ModuleResp> = vec![];
                    empty
                }
            };

            let module_vec: Vec<u8> =
                download_to_vec_from_modules(&handler, &modules[0].s3_key).await;
            let lockfile_contents_result = get_terraform_lockfile(&module_vec);
            assert_eq!(lockfile_contents_result.is_ok(), true);

            let terraform_tfvars_resul = get_terraform_tfvars(&module_vec);
            assert_eq!(terraform_tfvars_resul.is_err(), true); // This is desired because the module should not include any tfvars file

            assert_eq!(modules.len(), 1);
            assert_eq!(modules[0].module, "s3bucket");
            assert_eq!(modules[0].version, "0.1.2-dev+test.10");
            assert_eq!(modules[0].track, "dev");
            assert_eq!(modules[0].tf_extra_environment_variables.len(), 0);
            assert_eq!(modules[0].tf_variables.len(), 2);
            assert_eq!(
                modules[0]
                    .tf_providers
                    .iter()
                    .flat_map(|provider| provider.tf_variables.iter())
                    .count(),
                1
            );

            let examples = modules[0].clone().manifest.spec.examples.unwrap();
            assert_eq!(examples[0].name, "simple-bucket");
            assert_eq!(
                examples[0].variables.get("bucketName").unwrap(),
                "mybucket-14923"
            ); // specified as bucket_name in the manifest

            assert_eq!(examples[1].name, "advanced-bucket");
            assert_eq!(
                examples[1].variables.get("bucketName").unwrap(),
                "mybucket-14923"
            ); // specified as bucket_name in the manifest
            assert_eq!(
                examples[1]
                    .variables
                    .get("tags")
                    .unwrap()
                    .get("Name")
                    .unwrap(),
                "mybucket-14923"
            );
            assert_eq!(
                examples[1]
                    .variables
                    .get("tags")
                    .unwrap()
                    .get("Environment")
                    .unwrap(),
                "dev"
            );
            assert_eq!(examples.len(), 2);
        })
        .await;
    }

    #[tokio::test]
    async fn test_module_publish_s3bucket_defaults() {
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
                    .join("modules/s3bucket-defaults/")
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

            let modules = match handler.get_all_latest_module(&track).await {
                Ok(modules) => modules,
                Err(_e) => {
                    let empty: Vec<env_defs::ModuleResp> = vec![];
                    empty
                }
            };

            assert_eq!(modules[0].tf_variables.len(), 2);
            assert_eq!(
                modules[0]
                    .tf_providers
                    .iter()
                    .flat_map(|p| p.tf_variables.iter())
                    .count(),
                1
            );
            let nullable_with_default = modules[0]
                .tf_variables
                .iter()
                .find(|x| x.name == "nullable_with_default")
                .unwrap();
            assert_eq!(nullable_with_default.default, Some(serde_json::Value::Null));
            let nullable_with_default = modules[0]
                .tf_variables
                .iter()
                .find(|x| x.name == "nullable_without_default")
                .unwrap();
            assert_eq!(nullable_with_default.default, None);
        })
        .await;
    }

    #[tokio::test]
    async fn test_module_publish_3_s3bucket_versions() {
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

            for i in 0..3 {
                env_common::publish_module(
                    &handler,
                    &current_dir
                        .join("modules/s3bucket-dev/")
                        .to_str()
                        .unwrap()
                        .to_string(),
                    &"dev".to_string(),
                    Some(&format!("0.1.{}-dev", i)),
                    None,
                )
                .await
                .unwrap();
            }

            let module = "s3bucket".to_string();
            let track = "dev".to_string();

            let modules = match handler.get_all_module_versions(&module, &track).await {
                Ok(modules) => modules,
                Err(_e) => {
                    let empty: Vec<env_defs::ModuleResp> = vec![];
                    empty
                }
            };

            assert_eq!(modules.len(), 3);

            // Ensure same version cannot be published twice
            match env_common::publish_module(
                &handler,
                &current_dir
                    .join("modules/s3bucket-dev/")
                    .to_str()
                    .unwrap()
                    .to_string(),
                &"dev".to_string(),
                Some(&format!("0.1.{}-dev", 2)), // This version has already been published
                None,
            )
            .await
            {
                Ok(_) => assert_eq!(true, false),
                Err(_) => assert_eq!(true, true), // The expected behavior is to fail
            }
        })
        .await;
    }

    #[tokio::test]
    async fn test_module_publish_s3bucket_with_backup() {
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
                    .join("modules/s3bucket-with-backup/")
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

            let modules = match handler.get_all_latest_module(&track).await {
                Ok(modules) => modules,
                Err(_e) => {
                    let empty: Vec<env_defs::ModuleResp> = vec![];
                    empty
                }
            };

            assert_eq!(modules[0].tf_variables.len(), 2);
            assert_eq!(
                modules[0]
                    .tf_providers
                    .iter()
                    .flat_map(|p| p.tf_variables.iter().map(|v| v.name.clone()))
                    .collect::<HashSet<String>>()
                    .len(),
                1
            );
        })
        .await;
    }
}
