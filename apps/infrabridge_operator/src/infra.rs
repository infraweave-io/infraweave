use crate::module;

pub async fn mutate_infra(
    command: String,
    module: String,
    module_version: String,
    name: String,
    environment: String,
    deployment_id: String,
    variables: serde_json::value::Value,
    annotations: serde_json::value::Value,
) -> anyhow::Result<String> {
    let cloud = "aws";
    let cloud_handler: Box<dyn env_common::ModuleEnvironmentHandler + Send> = match cloud {
        "azure" => Box::new(env_common::AzureHandler {}),
        "aws" => Box::new(env_common::AwsHandler {}),
        _ => panic!("Invalid cloud provider"),
    };

    cloud_handler
        .mutate_infra(
            command,
            module,
            module_version,
            name,
            environment,
            deployment_id,
            variables,
            annotations,
        )
        .await
}
