use once_cell::sync::OnceCell;

use crate::interface::{AwsCloudHandler, AzureCloudHandler, CloudHandler};

pub static PROJECT_ID: OnceCell<String> = OnceCell::new();
pub static REGION: OnceCell<String> = OnceCell::new();

pub fn handler() -> impl CloudHandler {
    let aws = AwsCloudHandler {
        project_id: PROJECT_ID.get().unwrap().to_string(),
        region: REGION.get().unwrap().to_string(),
    };
    aws
    // let azure = AzureCloudHandler {
    //     project_id: PROJECT_ID.get().unwrap().to_string(),
    //     region: REGION.get().unwrap().to_string(),
    // };
    // azure
}

pub fn workload_handler(project_id: &str, region: &str) -> impl CloudHandler {
    let aws = AwsCloudHandler {
        project_id: project_id.to_string(),
        region: region.to_string(),
    };
    aws
}

pub fn central_handler() -> impl CloudHandler {
    let aws = AwsCloudHandler {
        project_id: "central".to_string(),
        region: REGION.get().unwrap().to_string(),
    };
    aws
}
