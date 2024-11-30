use aws_config::meta::region::RegionProviderChain;

pub async fn get_region() -> String {
    let region_provider = RegionProviderChain::default_provider().or_default_provider();
    let region = region_provider
        .region()
        .await
        .expect("Failed to load region");
    region.to_string()
}

// #[derive(PartialEq)]
// pub enum ModuleType {
//     Module,
//     Stack,
// }
