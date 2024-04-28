use aws_config::meta::region::RegionProviderChain;

pub async fn get_region() -> String {
    let region: String = match RegionProviderChain::default_provider().region().await {
        Some(d) => d.as_ref().to_string(),
        None => "eu-central-1".to_string(),
    };
    region
}
