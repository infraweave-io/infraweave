mod api;
mod backend;
pub mod direct_impl;
mod http_auth;
mod job_id;
pub mod local_bootstrap;
mod provider;
pub mod utils;

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
pub use http_auth::{
    call_authenticated_http, call_authenticated_http_raw, call_authenticated_http_with_config,
    get_aws_auth_context,
};
pub use job_id::get_current_job_id;
pub use provider::AwsCloudProvider;
pub use utils::get_region;

pub use utils::{
    get_bucket_name, get_bucket_name_for_region, get_table_name_for_region, DEFAULT_BUCKET_NAMES,
    DEFAULT_TABLE_NAMES,
};

pub use direct_impl::{
    download_file_as_bytes_direct, download_file_as_string_direct, generate_presigned_url_direct,
    get_environment_variables_direct, get_job_status_cross_account, insert_db_direct,
    publish_notification_direct, read_db_direct, read_logs_cross_account,
    start_runner_cross_account, transact_write_direct, upload_file_base64_direct,
    upload_file_url_direct,
};

pub use local_bootstrap::{bootstrap_dynamodb_tables, create_s3_buckets};
