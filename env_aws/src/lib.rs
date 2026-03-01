mod api;
mod backend;
#[cfg(feature = "direct")]
mod direct_impl;
mod http_auth;
mod http_client;
mod job_id;
mod provider;
mod utils;

pub use api::{
    // Alphabetical order and newlines between each function
    assume_role,
    get_all_deployments_query,
    get_all_events_between_query,
    get_all_latest_modules_query,
    get_all_latest_providers_query,
    get_all_latest_stacks_query,
    get_all_module_versions_query,
    get_all_policies_query,
    get_all_projects_query,
    get_all_regions_query,
    get_all_stack_versions_query,
    get_change_records_query,
    get_current_project_query,
    get_dependents_query,
    get_deployment_and_dependents_query,
    get_deployment_history_deleted_query,
    get_deployment_history_plans_query,
    get_deployment_query,
    get_deployments_to_driftcheck_query,
    get_deployments_using_module_query,
    get_events_query,
    get_latest_module_version_query,
    get_latest_provider_version_query,
    get_latest_stack_version_query,
    get_module_version_query,
    get_newest_policy_version_query,
    get_plan_deployment_query,
    get_policy_query,
    get_project_id,
    get_project_map_query,
    get_provider_version_query,
    get_stack_version_query,
    get_user_id,
    read_db,
    run_function,
};
pub use backend::set_backend;
pub use http_auth::{call_authenticated_http, call_authenticated_http_with_config};
pub use http_client::{
    http_deprecate_module, http_describe_deployment, http_download_provider,
    http_get_all_latest_modules, http_get_all_latest_providers, http_get_all_latest_stacks,
    http_get_all_projects, http_get_all_versions_for_module, http_get_all_versions_for_stack,
    http_get_change_record, http_get_deployments, http_get_job_status, http_get_logs,
    http_get_module_version, http_get_policies, http_get_policy_version,
    http_get_publish_job_status, http_get_stack_version, http_post, http_publish_module,
    is_http_mode_enabled,
};
pub use job_id::get_current_job_id;
pub use provider::AwsCloudProvider;
pub use utils::get_region;

#[cfg(feature = "direct")]
pub use utils::{DEFAULT_BUCKET_NAMES, DEFAULT_TABLE_NAMES};
