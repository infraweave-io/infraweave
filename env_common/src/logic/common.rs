use crate::interface::{AwsCloudHandler, CloudHandler};

pub fn handler() -> impl CloudHandler {
    let aws = AwsCloudHandler {};
    aws
}
