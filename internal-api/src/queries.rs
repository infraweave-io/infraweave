#[cfg(feature = "aws")]
pub use env_aws::{
    get_all_deployments_query, get_all_latest_modules_query, get_all_latest_providers_query,
    get_all_latest_stacks_query, get_all_module_versions_query, get_all_policies_query,
    get_all_projects_query, get_all_stack_versions_query, get_change_records_query,
    get_deployment_and_dependents_query, get_deployment_history_deleted_query,
    get_deployment_history_plans_query, get_deployments_using_module_query, get_events_query,
    get_module_version_query, get_plan_deployment_query, get_policy_query,
    get_provider_version_query, get_stack_version_query,
};

#[cfg(feature = "azure")]
pub use env_azure::{
    get_all_deployments_query, get_all_latest_modules_query, get_all_latest_providers_query,
    get_all_latest_stacks_query, get_all_module_versions_query, get_all_policies_query,
    get_all_projects_query, get_all_stack_versions_query, get_change_records_query,
    get_deployment_and_dependents_query, get_deployment_history_deleted_query,
    get_deployment_history_plans_query, get_deployments_using_module_query, get_events_query,
    get_module_version_query, get_plan_deployment_query, get_policy_query,
    get_provider_version_query, get_stack_version_query,
};
