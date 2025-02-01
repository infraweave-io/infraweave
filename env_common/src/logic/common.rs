use env_aws::AwsConfig;
use env_azure::AzureConfig;
use env_defs::GenericCloudConfig;
use once_cell::sync::OnceCell;

use crate::interface::{AwsCloudHandler, AzureCloudHandler, CloudHandler};

pub static PROJECT_ID: OnceCell<String> = OnceCell::new();
pub static REGION: OnceCell<String> = OnceCell::new();

pub fn handler() -> impl CloudHandler {
    let aws = AwsCloudHandler {
        project_id: PROJECT_ID.get().unwrap().to_string(),
        region: REGION.get().unwrap().to_string(),
        config: AwsConfig::default(),
    };
    aws
    // let azure = AzureCloudHandler {
    //     project_id: PROJECT_ID.get().unwrap().to_string(),
    //     region: REGION.get().unwrap().to_string(),
    // };
    // azure
}

pub fn workload_handler(project_id: &str, region: &str) -> impl CloudHandler {
    AwsCloudHandler {
        project_id: project_id.to_string(),
        region: region.to_string(),
        config: AwsConfig::default(),
    }
}

pub fn central_handler() -> impl CloudHandler {
    let aws = AwsCloudHandler {
        project_id: "central".to_string(),
        region: REGION.get().unwrap().to_string(),
        config: AwsConfig::default(),
    };
    aws
}

pub fn custom_handler(function_endpoint_url: &str) -> impl CloudHandler {
    // get env var provider to see if it is azure or aws
    // let provider = std::env::var("PROVIDER").expect("PROVIDER env var not set");

    let aws = AwsCloudHandler {
        project_id: PROJECT_ID.get().unwrap().to_string(),
        region: REGION.get().unwrap().to_string(),
        config: AwsConfig::custom(function_endpoint_url),
    };
    let azure = AzureCloudHandler {
        project_id: PROJECT_ID.get().unwrap().to_string(),
        region: REGION.get().unwrap().to_string(),
        config: AzureConfig::custom(function_endpoint_url),
    };
    aws
    // azure
}
