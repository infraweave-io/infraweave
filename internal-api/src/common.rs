use anyhow::{anyhow, Result};

/// Represents the cloud runtime environment
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloudRuntime {
    Aws,
    Azure,
}

impl CloudRuntime {
    /// Detect which cloud runtime we're running in
    pub fn detect() -> Self {
        // Check for AWS-specific environment variables
        if std::env::var("AWS_LAMBDA_FUNCTION_NAME").is_ok()
            || std::env::var("AWS_EXECUTION_ENV").is_ok()
        {
            return CloudRuntime::Aws;
        }

        // Check for Azure-specific environment variables
        if std::env::var("AZURE_FUNCTIONS_ENVIRONMENT").is_ok()
            || std::env::var("WEBSITE_INSTANCE_ID").is_ok()
            || std::env::var("AZURE_SUBSCRIPTION_ID").is_ok()
        {
            return CloudRuntime::Azure;
        }

        // Check explicit override
        if let Ok(cloud) = std::env::var("CLOUD_PROVIDER") {
            match cloud.to_lowercase().as_str() {
                "aws" => return CloudRuntime::Aws,
                "azure" => return CloudRuntime::Azure,
                _ => {}
            }
        }

        // Default to AWS for backwards compatibility
        CloudRuntime::Aws
    }

    /// Get a human-readable name for the runtime
    pub fn name(&self) -> &'static str {
        match self {
            CloudRuntime::Aws => "AWS",
            CloudRuntime::Azure => "Azure",
        }
    }
}

/// Helper function to get environment variable with error handling
pub fn get_env_var(name: &str) -> Result<String> {
    std::env::var(name).map_err(|_| anyhow!("Environment variable {} not found", name))
}
