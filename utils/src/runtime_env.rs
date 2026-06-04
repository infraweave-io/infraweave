pub fn dynamodb_endpoint() -> Option<String> {
    std::env::var("DYNAMODB_ENDPOINT")
        .or_else(|_| std::env::var("AWS_ENDPOINT_URL_DYNAMODB"))
        .ok()
}

pub fn s3_endpoint() -> Option<String> {
    std::env::var("AWS_ENDPOINT_URL_S3")
        .or_else(|_| std::env::var("MINIO_ENDPOINT"))
        .ok()
}

pub fn has_local_dynamodb() -> bool {
    dynamodb_endpoint().is_some()
}

pub fn has_local_s3() -> bool {
    s3_endpoint().is_some_and(|endpoint| is_local_s3_endpoint(&endpoint))
}

pub fn is_local_s3_endpoint(endpoint: &str) -> bool {
    endpoint.contains("127.0.0.1") || endpoint.contains("localhost") || endpoint.contains("minio")
}

pub fn has_local_azure_storage() -> bool {
    std::env::var("AZURE_STORAGE_CONNECTION_STRING")
        .or_else(|_| std::env::var("AZURITE_CONNECTION_STRING"))
        .ok()
        .map(|value| {
            value.contains("127.0.0.1")
                || value.contains("localhost")
                || value.contains("azurite")
                || value == "UseDevelopmentStorage=true"
        })
        .unwrap_or(false)
}

pub fn has_local_cosmos() -> bool {
    std::env::var("COSMOS_DB_ENDPOINT")
        .ok()
        .map(|value| {
            value.contains("127.0.0.1") || value.contains("localhost") || value.contains("cosmos")
        })
        .unwrap_or(false)
}

pub fn has_local_provider_endpoints() -> bool {
    has_local_dynamodb() || has_local_s3() || has_local_azure_storage() || has_local_cosmos()
}

pub fn is_running_in_ecs() -> bool {
    std::env::var("ECS_CONTAINER_METADATA_URI_V4")
        .or_else(|_| std::env::var("ECS_CONTAINER_METADATA_URI"))
        .is_ok()
}
