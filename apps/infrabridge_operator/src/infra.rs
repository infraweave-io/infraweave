pub async fn mutate_infra(
    event: String,
    module: String,
    name: String,
    environment: String,
    deployment_id: String,
    spec: serde_json::value::Value,
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
            event,
            module,
            name,
            environment,
            deployment_id,
            spec,
            annotations,
        )
        .await
}
