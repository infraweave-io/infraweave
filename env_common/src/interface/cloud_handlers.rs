use core::panic;

use async_trait::async_trait;
use env_defs::{
    ApiInfraPayload, Dependent, DeploymentResp, EnvironmentResp, EventData, InfraChangeRecord, ModuleResp, PolicyResp, ResourceResp
};
use serde_json::Value;

#[async_trait]
pub trait CloudHandler {
    async fn insert_db(&self) -> Result<(), anyhow::Error>;
    async fn transact_write(&self) -> Result<(), anyhow::Error>;
    async fn upload_file_base64(&self) -> Result<(), anyhow::Error>;
    async fn read_db(&self) -> Result<(), anyhow::Error>;
    async fn start_runner(&self) -> Result<(), anyhow::Error>;
    async fn read_logs(&self) -> Result<(), anyhow::Error>;
    async fn generate_presigned_url(&self) -> Result<(), anyhow::Error>;
}

pub struct AwsCloudHandler;
pub struct AzureCloudHandler;

#[async_trait]
impl CloudHandler for AwsCloudHandler {
    async fn insert_db(&self) -> Result<(), anyhow::Error> {
        panic!("Not implemented for AWS");
    }
    async fn transact_write(&self) -> Result<(), anyhow::Error> {
        panic!("Not implemented for AWS");
    }
    async fn upload_file_base64(&self) -> Result<(), anyhow::Error> {
        panic!("Not implemented for AWS");
    }
    async fn read_db(&self) -> Result<(), anyhow::Error> {
        panic!("Not implemented for AWS");
    }
    async fn start_runner(&self) -> Result<(), anyhow::Error> {
        panic!("Not implemented for AWS");
    }
    async fn read_logs(&self) -> Result<(), anyhow::Error> {
        panic!("Not implemented for AWS");
    }
    async fn generate_presigned_url(&self) -> Result<(), anyhow::Error> {
        panic!("Not implemented for AWS");
    }
}

#[async_trait]
impl CloudHandler for AzureCloudHandler {
    async fn insert_db(&self) -> Result<(), anyhow::Error> {
        panic!("Not implemented for Azure");
    }
    async fn transact_write(&self) -> Result<(), anyhow::Error> {
        panic!("Not implemented for Azure");
    }
    async fn upload_file_base64(&self) -> Result<(), anyhow::Error> {
        panic!("Not implemented for Azure");
    }
    async fn read_db(&self) -> Result<(), anyhow::Error> {
        panic!("Not implemented for Azure");
    }
    async fn start_runner(&self) -> Result<(), anyhow::Error> {
        panic!("Not implemented for Azure");
    }
    async fn read_logs(&self) -> Result<(), anyhow::Error> {
        panic!("Not implemented for Azure");
    }
    async fn generate_presigned_url(&self) -> Result<(), anyhow::Error> {
        panic!("Not implemented for Azure");
    }
}
