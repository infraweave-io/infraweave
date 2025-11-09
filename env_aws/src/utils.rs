use aws_config::meta::region::RegionProviderChain;

#[allow(dead_code)]
pub async fn get_region() -> String {
    let region_provider = RegionProviderChain::default_provider().or_default_provider();
    let region = match region_provider.region().await {
        Some(region) => region,
        None => {
            eprintln!("No region found, did you forget to set AWS_REGION?");
            std::process::exit(1);
        }
    };
    region.to_string()
}

// #[derive(PartialEq)]
// pub enum ModuleType {
//     Module,
//     Stack,
// }
