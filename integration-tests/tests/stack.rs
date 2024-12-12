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
    async fn test_module_publish_bucketcollection() {
        test_scaffold(|| async move {
            let current_dir = env::current_dir().expect("Failed to get current directory");

            env_common::publish_module(
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

            env_common::publish_module(
                &current_dir
                    .join("modules/s3bucket-dev/")
                    .to_str()
                    .unwrap()
                    .to_string(),
                &"dev".to_string(),
                Some("0.1.3-dev+test.10"),
            )
            .await
            .unwrap();

            env_common::publish_stack(
                &current_dir
                    .join("stacks/bucketcollection-dev/")
                    .to_str()
                    .unwrap()
                    .to_string(),
                &"dev".to_string(),
                Some("0.1.2-dev+test.10"),
            )
            .await
            .unwrap();

            let track = "".to_string();

            let stacks = match handler().get_all_latest_stack(&track).await {
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
                examples[0]
                    .variables
                    .get("bucket2")
                    .unwrap()
                    .get("tags")
                    .unwrap()
                    .get("SomeTag")
                    .unwrap(),
                "SomeValue",
            );
            assert_eq!(
                examples[0]
                    .variables
                    .get("bucket2")
                    .unwrap()
                    .get("tags")
                    .unwrap()
                    .get("AnotherTag")
                    .unwrap(),
                "ARN of dependency bucket {{ S3Bucket::bucket1a::bucketArn }}",
            );
        })
        .await;
    }
}
