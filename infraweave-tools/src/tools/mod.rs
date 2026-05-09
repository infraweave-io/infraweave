use crate::Tool;

mod archive_diff;
mod common;
mod deployments;
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
    ]
}
