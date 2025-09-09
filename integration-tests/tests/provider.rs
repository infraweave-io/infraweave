mod utils;
use utils::test_scaffold;

#[cfg(test)]
mod provider_tests {
    use super::*;
    use env_common::{download_provider_to_vec, interface::GenericCloudHandler};
    use env_defs::CloudProvider;
    use pretty_assertions::assert_eq;
    use std::env;

    #[tokio::test]
    async fn test_provder_publish_aws_5() {
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

            let providers = match handler.get_all_latest_provider().await {
                Ok(providers) => providers,
                Err(_e) => {
                    let empty: Vec<env_defs::ProviderResp> = vec![];
                    empty
                }
            };

            download_provider_to_vec(&handler, &providers[0].s3_key).await;

            assert_eq!(providers.len(), 1);
            assert_eq!(providers[0].name, "aws-5");
            assert_eq!(providers[0].version, "0.1.2");
            assert_eq!(providers[0].tf_extra_environment_variables.len(), 15);
            assert_eq!(providers[0].tf_variables.len(), 1);
        })
        .await;
    }

    #[tokio::test]
    async fn test_provder_publish_aws_5_us_east_1() {
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

            let providers = match handler.get_all_latest_provider().await {
                Ok(providers) => providers,
                Err(_e) => {
                    let empty: Vec<env_defs::ProviderResp> = vec![];
                    empty
                }
            };

            download_provider_to_vec(&handler, &providers[0].s3_key).await;

            assert_eq!(providers.len(), 1);
            assert_eq!(providers[0].name, "aws-5-us-east-1");
            assert_eq!(providers[0].manifest.spec.alias.clone().unwrap(), "us-east-1");
            assert_eq!(providers[0].version, "0.1.2");
            assert_eq!(providers[0].tf_extra_environment_variables.len(), 15);
            assert_eq!(providers[0].tf_variables.len(), 1);
        })
        .await;
    }
}
