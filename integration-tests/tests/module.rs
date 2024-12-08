mod utils;
use utils::test_scaffold;

#[cfg(test)]
mod module_tests {
    use super::*;
    use env_common::interface::CloudHandler;
    use env_common::logic::handler;
    use pretty_assertions::assert_eq;
    use std::env;

    #[tokio::test]
    async fn test_module_publish_s3bucket() {
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

            let track = "".to_string();

            let modules = match handler().get_all_latest_module(&track).await {
                Ok(modules) => modules,
                Err(_e) => {
                    let empty: Vec<env_defs::ModuleResp> = vec![];
                    empty
                }
            };

            assert_eq!(modules.len(), 1);
            assert_eq!(modules[0].module, "s3bucket");
            assert_eq!(modules[0].version, "0.1.2-dev+test.10");
            assert_eq!(modules[0].track, "dev");

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
    async fn test_module_publish_10_s3bucket_versions() {
        test_scaffold(|| async move {
            let current_dir = env::current_dir().expect("Failed to get current directory");

            for i in 0..10 {
                env_common::publish_module(
                    &current_dir
                        .join("modules/s3bucket/")
                        .to_str()
                        .unwrap()
                        .to_string(),
                    &"dev".to_string(),
                    Some(&format!("0.1.{}-dev", i)),
                )
                .await
                .unwrap();
            }

            let module = "s3bucket".to_string();
            let track = "dev".to_string();

            let modules = match handler().get_all_module_versions(&module, &track).await {
                Ok(modules) => modules,
                Err(_e) => {
                    let empty: Vec<env_defs::ModuleResp> = vec![];
                    empty
                }
            };

            assert_eq!(modules.len(), 10);

            // Ensure same version cannot be published twice
            match env_common::publish_module(
                &current_dir
                    .join("modules/s3bucket/")
                    .to_str()
                    .unwrap()
                    .to_string(),
                &"dev".to_string(),
                Some(&format!("0.1.{}-dev", 5)), // This version has already been published
            )
            .await
            {
                Ok(_) => assert_eq!(true, false),
                Err(_) => assert_eq!(true, true), // The expected behavior is to fail
            }
        })
        .await;
    }
}
