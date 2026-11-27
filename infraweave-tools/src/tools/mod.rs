use crate::Tool;

mod archive_diff;
mod changes;
mod common;
mod deployments;
mod jobs;
mod modules;
mod projects;
mod stacks;

/// The full curated tool registry used by both the chat backend and the MCP
/// server. Add new tools here as they're implemented.
pub fn registry() -> Vec<Box<dyn Tool>> {
    vec![
        Box::new(modules::ListModules),
        Box::new(modules::DescribeModule),
        Box::new(modules::DiffModuleVersions),
        Box::new(stacks::ListStacks),
        Box::new(stacks::DescribeStack),
        Box::new(stacks::DiffStackVersions),
        Box::new(deployments::ListDeployments),
        Box::new(deployments::DebugDeployment),
        Box::new(projects::ListProjects),
        Box::new(jobs::GetDeploymentLogs),
        Box::new(jobs::GetJobStatus),
        Box::new(changes::ListChangeRecords),
        Box::new(changes::DescribeChangeRecord),
    ]
}

#[cfg(test)]
mod tests {
    use super::registry;

    /// Prompt caching is a prefix match over `tools -> system -> messages`.
    /// Reordering the registry silently destroys the cache hit rate, so the
    /// order is part of the contract.
    #[test]
    fn registry_order_is_stable() {
        let names: Vec<&str> = registry().iter().map(|t| t.def().name).collect();
        assert_eq!(
            names,
            vec![
                "list_modules",
                "describe_module",
                "diff_module_versions",
                "list_stacks",
                "describe_stack",
                "diff_stack_versions",
                "list_deployments",
                "debug_deployment",
                "list_projects",
                "get_deployment_logs",
                "get_job_status",
                "list_change_records",
                "describe_change_record",
            ]
        );
    }
}
