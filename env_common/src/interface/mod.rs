mod cloud_handlers;
mod deployment_status_handler;
#[cfg(test)]
mod mock_cloud_provider;
mod no_cloud_provider;

pub use cloud_handlers::{
    get_current_identity, get_region_env_var, initialize_project_id_and_region, GenericCloudHandler,
};
pub use deployment_status_handler::DeploymentStatusHandler;

pub use no_cloud_provider::NoCloudProvider;

#[cfg(test)]
pub use mock_cloud_provider::MockTestCloudProvider as TestCloudProvider;
