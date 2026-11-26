mod utils;
use integration_tests::shared::{shared_scaffold, Fixture};
use utils::test_scaffold;

#[cfg(test)]
mod module_tests {
    use super::*;
    use env_common::download_to_vec_from_modules;
    use env_defs::CloudProvider;
    use env_utils::{get_terraform_lockfile, get_terraform_tfvars};
    use pretty_assertions::assert_eq;
    use std::{collections::HashSet, env};

    #[tokio::test]
    async fn test_module_publish_s3bucket_invalid_provider() {
        test_scaffold(|ctx| async move {
            let handler = ctx.api_handler.clone();
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

            assert!(publish_attempt.is_err());
            let modules = handler.get_all_latest_module("").await.unwrap_or_default();
            assert!(
                !modules.iter().any(|m| m.module == "s3bucket"),
                "module must not be published without its provider"
            );
        })
        .await;
    }

    #[tokio::test]
    async fn test_module_publish_s3bucket() {
        shared_scaffold(Fixture::Providers, |ctx| async move {
            let handler = ctx.api_handler.clone();
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
                None,
            )
            .await
            .unwrap();

            let modules = handler.get_all_latest_module("").await.unwrap_or_default();
            let module = modules
                .iter()
                .find(|m| m.module == "s3bucket")
                .expect("module s3bucket was not published");

            let module_vec: Vec<u8> = download_to_vec_from_modules(&handler, &module.s3_key).await;
            let lockfile_contents_result = get_terraform_lockfile(&module_vec);
            assert_eq!(lockfile_contents_result.is_ok(), true);

            let terraform_tfvars_resul = get_terraform_tfvars(&module_vec);
            assert_eq!(terraform_tfvars_resul.is_err(), true); // This is desired because the module should not include any tfvars file

            assert_eq!(module.version, "0.1.2-dev+test.10");
            assert_eq!(module.track, "dev");
            assert_eq!(module.tf_extra_environment_variables.len(), 0);
            assert_eq!(module.tf_variables.len(), 2);
            assert_eq!(
                module
                    .tf_providers
                    .iter()
                    .flat_map(|provider| provider.tf_variables.iter())
                    .count(),
                1
            );

            let examples = module.clone().manifest.spec.examples.unwrap();
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
        shared_scaffold(Fixture::Providers, |ctx| async move {
            let handler = ctx.api_handler.clone();
            let current_dir = env::current_dir().expect("Failed to get current directory");

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

            let modules = handler.get_all_latest_module("").await.unwrap_or_default();
            let module = modules
                .iter()
                .find(|m| m.module == "s3bucketdefaults")
                .expect("module s3bucketdefaults was not published");

            assert_eq!(module.tf_variables.len(), 2);
            assert_eq!(
                module
                    .tf_providers
                    .iter()
                    .flat_map(|p| p.tf_variables.iter())
                    .count(),
                1
            );
            let nullable_with_default = module
                .tf_variables
                .iter()
                .find(|x| x.name == "nullable_with_default")
                .unwrap();
            assert_eq!(nullable_with_default.default, Some(serde_json::Value::Null));
            let nullable_with_default = module
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
        shared_scaffold(Fixture::Providers, |ctx| async move {
            let handler = ctx.api_handler.clone();
            let current_dir = env::current_dir().expect("Failed to get current directory");

            // s3bucket-simple: no other test publishes it on this backend.
            let module_dir = current_dir.join("modules/s3bucket-simple/");
            let fixture_path = module_dir.to_str().unwrap();
            for i in 0..3 {
                env_common::publish_module(
                    &handler,
                    fixture_path,
                    "dev",
                    Some(&format!("0.1.{}-dev", i)),
                    None,
                )
                .await
                .unwrap();
            }

            let published_versions = || async {
                handler
                    .get_all_module_versions("s3bucketsimple", "dev")
                    .await
                    .unwrap_or_default()
                    .into_iter()
                    .map(|m| m.version)
                    .collect::<HashSet<String>>()
            };
            let expected_versions: HashSet<String> = ["0.1.0-dev", "0.1.1-dev", "0.1.2-dev"]
                .into_iter()
                .map(String::from)
                .collect();
            assert_eq!(published_versions().await, expected_versions);

            // Ensure same version cannot be published twice
            let republish =
                env_common::publish_module(&handler, fixture_path, "dev", Some("0.1.2-dev"), None)
                    .await;
            assert!(republish.is_err(), "republishing 0.1.2-dev must fail");
            assert_eq!(published_versions().await, expected_versions);
        })
        .await;
    }

    #[tokio::test]
    async fn test_module_publish_s3bucket_with_backup() {
        shared_scaffold(Fixture::Providers, |ctx| async move {
            let handler = ctx.api_handler.clone();
            let current_dir = env::current_dir().expect("Failed to get current directory");

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

            let modules = handler.get_all_latest_module("").await.unwrap_or_default();
            let module = modules
                .iter()
                .find(|m| m.module == "s3bucketwithbackup")
                .expect("module s3bucketwithbackup was not published");

            assert_eq!(module.tf_variables.len(), 2);
            assert_eq!(
                module
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
