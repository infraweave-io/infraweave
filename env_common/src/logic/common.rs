use once_cell::sync::OnceCell;

use crate::interface::{AwsCloudHandler, CloudHandler};

pub static PROJECT_ID: OnceCell<String> = OnceCell::new();

pub fn handler() -> impl CloudHandler {
    let aws = AwsCloudHandler {
        project_id: PROJECT_ID.get().unwrap().to_string(),
        region: "eu-central-1".to_string(),
    };
    aws
}
