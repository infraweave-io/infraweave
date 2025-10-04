use std::collections::BTreeMap;

pub const UNGROUPED_KEY: &str = "_other";
pub const TREE_BRANCH: &str = "├─";
pub const TREE_LAST: &str = "└─";
pub const TREE_VERTICAL: &str = "│";
pub const TREE_FOLDER_ICON: &str = "📁";

#[derive(Debug, Clone, PartialEq)]
pub enum NavItem {
    General,
    Composition,
    VariablesHeader,
    VariableFolder {
        module_name: String,
    },
    Variable {
        module_name: Option<String>,
        name: String,
        is_required: bool,
    },
    OutputsHeader,
    OutputFolder {
        module_name: String,
    },
    Output {
        module_name: Option<String>,
        name: String,
    },
    Dependencies,
    PolicyResults,
    Logs,
}

impl NavItem {
    pub fn display_string(&self) -> String {
        match self {
            NavItem::General => "📋 General".to_string(),
            NavItem::Composition => "🧩 Composition".to_string(),
            NavItem::VariablesHeader => "🔧 Variables".to_string(),
            NavItem::VariableFolder { module_name } => {
                format!(
                    "  {} {} {}",
                    TREE_BRANCH,
                    TREE_FOLDER_ICON,
                    to_camel_case(module_name)
                )
            }
            NavItem::Variable {
                module_name: Some(_),
                name,
                is_required,
            } => {
                let parts: Vec<&str> = name.split("__").collect();
                let var_name = if parts.len() >= 2 {
                    parts[1..].join("__")
                } else {
                    name.clone()
                };
                let required_marker = if *is_required { "* " } else { "" };
                format!(
                    "  {}  {} {}{}",
                    TREE_VERTICAL,
                    TREE_BRANCH,
                    required_marker,
                    to_camel_case(&var_name)
                )
            }
            NavItem::Variable {
                module_name: None,
                name,
                is_required,
            } => {
                let required_marker = if *is_required { "* " } else { "" };
                format!("  {} {}{}", TREE_LAST, required_marker, to_camel_case(name))
            }
            NavItem::OutputsHeader => "📤 Outputs".to_string(),
            NavItem::OutputFolder { module_name } => {
                format!(
                    "  {} {} {}",
                    TREE_BRANCH,
                    TREE_FOLDER_ICON,
                    to_camel_case(module_name)
                )
            }
            NavItem::Output {
                module_name: Some(_),
                name,
            } => {
                let parts: Vec<&str> = name.split("__").collect();
                let output_name = if parts.len() >= 2 {
                    parts[1..].join("__")
                } else {
                    name.clone()
                };
                format!(
                    "  {}  {} {}",
                    TREE_VERTICAL,
                    TREE_BRANCH,
                    to_camel_case(&output_name)
                )
            }
            NavItem::Output {
                module_name: None,
                name,
            } => {
                format!("  {} {}", TREE_LAST, to_camel_case(name))
            }
            NavItem::Dependencies => "🔗 Dependencies".to_string(),
            NavItem::PolicyResults => "📊 Policy Results".to_string(),
            NavItem::Logs => "📝 Logs".to_string(),
        }
    }

    pub fn title(&self) -> String {
        match self {
            NavItem::General => "General Information".to_string(),
            NavItem::Composition => "Composition".to_string(),
            NavItem::VariablesHeader => "All Variables".to_string(),
            NavItem::VariableFolder { module_name } => {
                format!("{} Variables", to_camel_case(module_name))
            }
            NavItem::Variable { name, .. } => {
                let parts: Vec<&str> = name.split("__").collect();
                if parts.len() >= 2 {
                    to_camel_case(&parts[1..].join("__"))
                } else {
                    to_camel_case(name)
                }
            }
            NavItem::OutputsHeader => "All Outputs".to_string(),
            NavItem::OutputFolder { module_name } => {
                format!("{} Outputs", to_camel_case(module_name))
            }
            NavItem::Output { name, .. } => {
                let parts: Vec<&str> = name.split("__").collect();
                if parts.len() >= 2 {
                    to_camel_case(&parts[1..].join("__"))
                } else {
                    to_camel_case(name)
                }
            }
            NavItem::Dependencies => "Dependencies".to_string(),
            NavItem::PolicyResults => "Policy Results".to_string(),
            NavItem::Logs => "Logs".to_string(),
        }
    }
}

pub struct GroupedItems<'a, T> {
    pub grouped: BTreeMap<String, Vec<&'a T>>,
    pub total_items: usize,
    pub folder_count: usize,
}

/// Groups items by module prefix (before "__").
/// Items without "__" are grouped under UNGROUPED_KEY.
pub fn group_terraform_items<T>(
    items: &[T],
    name_extractor: impl Fn(&T) -> &str,
) -> GroupedItems<'_, T> {
    let mut grouped: BTreeMap<String, Vec<&T>> = BTreeMap::new();

    for item in items {
        let name = name_extractor(item);
        let parts: Vec<&str> = name.split("__").collect();

        let module_name = if parts.len() >= 2 {
            parts[0].to_string()
        } else {
            UNGROUPED_KEY.to_string()
        };

        grouped
            .entry(module_name)
            .or_insert_with(Vec::new)
            .push(item);
    }

    let total_items = items.len();
    let folder_count = grouped.len();

    GroupedItems {
        grouped,
        total_items,
        folder_count,
    }
}

pub fn count_nav_items_for_grouped<T>(items: &[T], name_extractor: impl Fn(&T) -> &str) -> usize {
    if items.is_empty() {
        return 0;
    }

    let grouped = group_terraform_items(items, name_extractor);
    1 + grouped.folder_count + grouped.total_items
}

pub fn to_camel_case(snake_case: &str) -> String {
    let mut result = String::new();
    let mut capitalize_next = false;

    for c in snake_case.chars() {
        if c == '_' {
            capitalize_next = true;
        } else if capitalize_next {
            result.push(c.to_ascii_uppercase());
            capitalize_next = false;
        } else {
            result.push(c);
        }
    }

    result
}

pub fn is_variable_required(var: &env_defs::TfVariable) -> bool {
    if var.default.is_none() {
        return true;
    }

    if !var.nullable && var.default == Some(serde_json::Value::Null) {
        return true;
    }

    false
}

pub fn build_stack_nav_items(stack: &env_defs::ModuleResp) -> Vec<NavItem> {
    let mut items = vec![NavItem::General];

    if let Some(stack_data) = &stack.stack_data {
        if !stack_data.modules.is_empty() {
            items.push(NavItem::Composition);
        }
    }

    if !stack.tf_variables.is_empty() {
        items.push(NavItem::VariablesHeader);

        let grouped = group_terraform_items(&stack.tf_variables, |v| &v.name);

        for (module_name, vars) in &grouped.grouped {
            if module_name == UNGROUPED_KEY {
                for var in vars {
                    items.push(NavItem::Variable {
                        module_name: None,
                        name: var.name.clone(),
                        is_required: is_variable_required(var),
                    });
                }
            } else {
                items.push(NavItem::VariableFolder {
                    module_name: module_name.clone(),
                });
                for var in vars {
                    items.push(NavItem::Variable {
                        module_name: Some(module_name.clone()),
                        name: var.name.clone(),
                        is_required: is_variable_required(var),
                    });
                }
            }
        }
    }

    if !stack.tf_outputs.is_empty() {
        items.push(NavItem::OutputsHeader);

        let grouped = group_terraform_items(&stack.tf_outputs, |o| &o.name);

        for (module_name, outputs) in &grouped.grouped {
            if module_name == UNGROUPED_KEY {
                for output in outputs {
                    items.push(NavItem::Output {
                        module_name: None,
                        name: output.name.clone(),
                    });
                }
            } else {
                items.push(NavItem::OutputFolder {
                    module_name: module_name.clone(),
                });
                for output in outputs {
                    items.push(NavItem::Output {
                        module_name: Some(module_name.clone()),
                        name: output.name.clone(),
                    });
                }
            }
        }
    }

    items
}

pub fn build_module_nav_items(module: &env_defs::ModuleResp) -> Vec<NavItem> {
    let mut items = vec![NavItem::General];

    if !module.tf_variables.is_empty() {
        items.push(NavItem::VariablesHeader);
        for var in &module.tf_variables {
            items.push(NavItem::Variable {
                module_name: None,
                name: var.name.clone(),
                is_required: is_variable_required(var),
            });
        }
    }

    if !module.tf_outputs.is_empty() {
        items.push(NavItem::OutputsHeader);
        for output in &module.tf_outputs {
            items.push(NavItem::Output {
                module_name: None,
                name: output.name.clone(),
            });
        }
    }

    items
}

pub fn build_deployment_nav_items(deployment: &env_defs::DeploymentResp) -> Vec<NavItem> {
    let mut items = vec![NavItem::General];

    if !deployment.variables.is_null() && deployment.variables.is_object() {
        if let Some(obj) = deployment.variables.as_object() {
            if !obj.is_empty() {
                items.push(NavItem::VariablesHeader);
            }
        }
    }

    if !deployment.output.is_null() && deployment.output.is_object() {
        if let Some(obj) = deployment.output.as_object() {
            if !obj.is_empty() {
                items.push(NavItem::OutputsHeader);
            }
        }
    }

    if !deployment.dependencies.is_empty() {
        items.push(NavItem::Dependencies);
    }

    if !deployment.policy_results.is_empty() {
        items.push(NavItem::PolicyResults);
    }

    items.push(NavItem::Logs);

    items
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_to_camel_case() {
        assert_eq!(to_camel_case("hello_world"), "helloWorld");
        assert_eq!(to_camel_case("test"), "test");
        assert_eq!(to_camel_case("a_b_c"), "aBC");
    }

    #[test]
    fn test_group_terraform_items() {
        struct Item {
            name: String,
        }

        let items = vec![
            Item {
                name: "module1__var1".to_string(),
            },
            Item {
                name: "module1__var2".to_string(),
            },
            Item {
                name: "module2__var1".to_string(),
            },
            Item {
                name: "standalone".to_string(),
            },
        ];

        let grouped = group_terraform_items(&items, |item| &item.name);

        assert_eq!(grouped.grouped.len(), 3);
        assert_eq!(grouped.total_items, 4);
        assert_eq!(grouped.folder_count, 3);
        assert_eq!(grouped.grouped.get("module1").unwrap().len(), 2);
        assert_eq!(grouped.grouped.get("module2").unwrap().len(), 1);
        assert_eq!(grouped.grouped.get(UNGROUPED_KEY).unwrap().len(), 1);
    }
}
