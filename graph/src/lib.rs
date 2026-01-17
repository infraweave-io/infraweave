use anyhow::{Context, Result};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

#[derive(Deserialize, Debug)]
pub struct Plan {
    pub resource_changes: Option<Vec<ResourceChange>>,
    pub output_changes: Option<HashMap<String, Change>>,
    pub configuration: Option<Configuration>,
    pub prior_state: Option<State>,
    pub planned_values: Option<State>,
    pub values: Option<StateValues>, // For direct State file parsing
}

#[derive(Deserialize, Debug)]
pub struct State {
    pub values: Option<StateValues>,
}

#[derive(Deserialize, Debug)]
pub struct StateValues {
    pub root_module: StateModule,
    pub outputs: Option<HashMap<String, StateOutput>>,
}

#[derive(Deserialize, Debug)]
pub struct StateOutput {
    pub sensitive: bool,
    pub value: Option<serde_json::Value>,
}

#[derive(Deserialize, Debug)]
pub struct StateModule {
    pub resources: Option<Vec<StateResource>>,
    pub child_modules: Option<Vec<StateModule>>,
}

#[derive(Deserialize, Debug)]
pub struct StateResource {
    pub address: String,
    pub mode: String,
    #[serde(rename = "type")]
    pub resource_type: String,
    pub values: Option<serde_json::Value>,
}

#[derive(Deserialize, Debug)]
pub struct Configuration {
    pub root_module: ModuleConfig,
}

#[derive(Deserialize, Debug)]
pub struct ModuleConfig {
    pub resources: Option<Vec<ResourceConfig>>,
    pub module_calls: Option<HashMap<String, ModuleCall>>,
    pub outputs: Option<HashMap<String, OutputConfig>>,
}

#[derive(Deserialize, Debug)]
pub struct OutputConfig {
    pub expression: Option<serde_json::Value>,
}

#[derive(Deserialize, Debug)]
pub struct ModuleCall {
    pub module: Option<ModuleConfig>,
}

#[derive(Deserialize, Debug)]
pub struct ResourceConfig {
    pub address: String,
    pub expressions: Option<HashMap<String, serde_json::Value>>,
    pub count_expression: Option<serde_json::Value>,
    pub for_each_expression: Option<serde_json::Value>,
}

#[derive(Deserialize, Debug)]
pub struct ResourceChange {
    pub address: String,
    #[serde(rename = "type")]
    pub resource_type: String,
    pub mode: Option<String>,
    pub change: Change,
}

#[derive(Deserialize, Debug)]
pub struct Change {
    pub actions: Vec<String>,
    pub after: Option<serde_json::Value>,
    #[serde(default)]
    pub after_unknown: Option<serde_json::Value>,
    #[serde(default)]
    pub after_sensitive: Option<serde_json::Value>,
}

#[derive(Serialize, Debug, Clone)]
pub struct OutputNodeData {
    pub label: String,
    #[serde(rename = "type")]
    pub node_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub action: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hcl: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub values: Option<serde_json::Value>,
}

#[derive(Serialize, Debug, Clone)]
pub struct OutputNodeStyle {
    #[serde(rename = "backgroundColor")]
    pub background_color: String,
    pub border: String,
    #[serde(rename = "zIndex")]
    pub z_index: i32,
}

#[derive(Serialize, Debug, Clone)]
pub struct OutputNodePosition {
    pub x: i32,
    pub y: i32,
}

#[derive(Serialize, Debug, Clone)]
#[serde(tag = "type")]
#[serde(rename_all = "camelCase")]
pub enum OutputNode {
    Group {
        id: String,
        data: OutputNodeData,
        position: OutputNodePosition,
        style: OutputNodeStyle,
        #[serde(skip_serializing_if = "Option::is_none")]
        #[serde(rename = "parentId")]
        parent_id: Option<String>,
    },
    #[serde(untagged)]
    Resource {
        id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        #[serde(rename = "parentId")]
        parent_id: Option<String>,
        data: OutputNodeData,
        position: OutputNodePosition,
    },
}

#[derive(Serialize, Debug)]
pub struct OutputEdge {
    pub id: String,
    pub source: String,
    pub target: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attributes: Option<Vec<String>>,
}

#[derive(Serialize, Debug)]
pub struct OutputGraph {
    pub nodes: Vec<OutputNode>,
    pub edges: Vec<OutputEdge>,
}

fn determine_block_type(address: &str, is_data: bool) -> String {
    // Check for explicit prefixes first
    if address.starts_with("var.") {
        return "var".to_string();
    }
    if address.starts_with("local.") {
        return "local".to_string();
    }
    if address.starts_with("output.") {
        return "output".to_string();
    }

    // Check for keys in address parts (module inputs/outputs/locals)
    let parts: Vec<&str> = address.split('.').collect();
    for part in &parts {
        if *part == "var" {
            return "var".to_string();
        }
        if *part == "local" {
            return "local".to_string();
        }
        if *part == "output" {
            return "output".to_string();
        }
    }

    if is_data {
        return "data".to_string();
    }

    if parts.len() >= 3 && parts[parts.len() - 3] == "data" {
        return "data".to_string();
    }
    if address.starts_with("data.") {
        return "data".to_string();
    }

    "resource".to_string()
}

// Returns the deepest module parent if found
fn extract_parent_modules(
    address: &str,
    known_modules: &mut HashSet<String>,
    nodes: &mut Vec<OutputNode>,
) -> Option<String> {
    // address: module.a.module.b.resource
    // splits: [module, a, module, b, resource]
    // We strictly look for "module.name" patterns.

    let parts: Vec<&str> = address.split('.').collect();
    let mut current_path = String::new();
    let mut hierarchy = Vec::new(); // List of module paths found

    let mut i = 0;
    while i < parts.len() {
        if parts[i] == "module" && i + 1 < parts.len() {
            if !current_path.is_empty() {
                current_path.push('.');
            }
            current_path.push_str("module.");
            current_path.push_str(parts[i + 1]);
            hierarchy.push(current_path.clone());
            i += 2;
        } else {
            // Just part of resource name
            if !current_path.is_empty() {
                current_path.push('.');
            }
            current_path.push_str(parts[i]);
            i += 1;
        }
    }

    // Now we have a list of modules in hierarchy e.g. ["module.vpc", "module.vpc.module.extras"]
    if hierarchy.is_empty() {
        return None;
    }

    for (idx, module_id) in hierarchy.iter().enumerate() {
        if !known_modules.contains(module_id) {
            known_modules.insert(module_id.clone());

            let parent_id = if idx > 0 {
                Some(hierarchy[idx - 1].clone())
            } else {
                None
            };

            nodes.push(OutputNode::Group {
                id: module_id.clone(),
                data: OutputNodeData {
                    label: module_id.clone(),
                    node_type: "module".to_string(),
                    action: None,
                    count: None, // Group has no count
                    values: None,
                    hcl: None,
                },
                position: OutputNodePosition { x: 0, y: 0 },
                style: OutputNodeStyle {
                    background_color: "rgba(56, 139, 253, 0.05)".to_string(),
                    border: "1px dashed #388bfd".to_string(),
                    z_index: -1,
                },
                parent_id,
            });
        }
    }

    hierarchy.last().cloned()
}

fn process_values(
    after: Option<&serde_json::Value>,
    unknown: Option<&serde_json::Value>,
    sensitive: Option<&serde_json::Value>,
) -> Option<serde_json::Value> {
    if let Some(serde_json::Value::Bool(true)) = sensitive {
        return Some(serde_json::Value::String("(sensitive)".to_string()));
    }
    if let Some(serde_json::Value::Bool(true)) = unknown {
        return Some(serde_json::Value::String("(known after apply)".to_string()));
    }

    // Determine type based on available data
    let is_object = after.map_or(false, |v| v.is_object())
        || unknown.map_or(false, |v| v.is_object())
        || sensitive.map_or(false, |v| v.is_object());

    let is_array = after.map_or(false, |v| v.is_array())
        || unknown.map_or(false, |v| v.is_array())
        || sensitive.map_or(false, |v| v.is_array());

    if is_object {
        let mut keys = HashSet::new();
        if let Some(serde_json::Value::Object(m)) = after {
            keys.extend(m.keys());
        }
        if let Some(serde_json::Value::Object(m)) = unknown {
            keys.extend(m.keys());
        }
        if let Some(serde_json::Value::Object(m)) = sensitive {
            keys.extend(m.keys());
        }

        let mut new_map = serde_json::Map::new();
        for k in keys {
            let next_a = after.and_then(|v| v.get(k));
            let next_u = unknown.and_then(|v| v.get(k));
            let next_s = sensitive.and_then(|v| v.get(k));

            if let Some(processed) = process_values(next_a, next_u, next_s) {
                new_map.insert(k.clone(), processed);
            }
        }
        return Some(serde_json::Value::Object(new_map));
    }

    if is_array {
        let len_a = after.and_then(|v| v.as_array()).map_or(0, |a| a.len());
        let len_u = unknown.and_then(|v| v.as_array()).map_or(0, |a| a.len());
        let len_s = sensitive.and_then(|v| v.as_array()).map_or(0, |a| a.len());
        let max_len = len_a.max(len_u).max(len_s);

        let mut new_arr = Vec::new();
        for i in 0..max_len {
            let next_a = after.and_then(|v| v.get(i));
            let next_u = unknown.and_then(|v| v.get(i));
            let next_s = sensitive.and_then(|v| v.get(i));

            if let Some(processed) = process_values(next_a, next_u, next_s) {
                new_arr.push(processed);
            } else {
                new_arr.push(serde_json::Value::Null);
            }
        }
        return Some(serde_json::Value::Array(new_arr));
    }

    if let Some(val) = after {
        return Some(val.clone());
    }

    None
}

fn create_node(
    address: String,
    resource_map: &HashMap<String, Vec<&ResourceChange>>,
    output_map: &HashMap<String, Change>,
    state_values_map: &HashMap<String, serde_json::Value>,
    known_modules: &mut HashSet<String>,
    nodes: &mut Vec<OutputNode>,
    include_values: bool,
    hcl: Option<String>,
    active_plan_addresses: &HashSet<String>,
) -> Option<OutputNode> {
    // Filter out noise nodes
    if address == "root" || address.starts_with("provider[") || address.starts_with("meta.") {
        return None;
    }

    // Check if implicitly hidden (not in plan).
    // Apply stricter filtering for data sources and resources that aren't in the resource_map
    // or active_plan_addresses to exclude nodes not relevant to the current operation.

    let parent_id = extract_parent_modules(&address, known_modules, nodes);

    // If the node IS the module group itself, skip creating a resource for it.
    // This removes the explicitly graphed module nodes (e.g. "module.vpc") which are noise.
    if let Some(pid) = &parent_id {
        if pid == &address {
            return None;
        }
    }

    // Determine Action and Type
    let (action, is_data, count, values) = if let Some(changes) = resource_map.get(&address) {
        let mut distinct_actions = HashSet::new();
        let mut is_data_mode = false;
        let mut collected_values = Vec::new();

        for change in changes {
            if change.mode.as_deref() == Some("data") {
                is_data_mode = true;
            }
            // Flatten actions (e.g. ["create", "delete"] -> inserts both)
            for act in &change.change.actions {
                distinct_actions.insert(act.clone());
            }

            if include_values {
                if let Some(merged) = process_values(
                    change.change.after.as_ref(),
                    change.change.after_unknown.as_ref(),
                    change.change.after_sensitive.as_ref(),
                ) {
                    collected_values.push(merged);
                }
            }
        }

        // Remove "no-op" if there are other actions (noise reduction)
        if distinct_actions.len() > 1 && distinct_actions.contains("no-op") {
            distinct_actions.remove("no-op");
        }

        let mut actions_vec: Vec<String> = distinct_actions.into_iter().collect();
        actions_vec.sort();
        let action_str = actions_vec.join(", ");

        let count_val = if changes.len() > 1 || changes.iter().any(|c| c.address.ends_with(']')) {
            Some(changes.len())
        } else {
            None
        };

        let values_val = if include_values && !collected_values.is_empty() {
            if collected_values.len() == 1 {
                Some(collected_values[0].clone())
            } else {
                Some(serde_json::Value::Array(collected_values))
            }
        } else {
            None
        };

        (Some(action_str), is_data_mode, count_val, values_val)
    } else {
        // Fallback: Check state_values_map (for existing infrastructure view)
        if let Some(state_val) = state_values_map.get(&address) {
            let temp_type = determine_block_type(&address, false);
            if temp_type == "data" {
                (
                    Some("read".to_string()),
                    true,
                    None,
                    if include_values {
                        Some(state_val.clone())
                    } else {
                        None
                    },
                )
            } else {
                // Managed resource in state -> No action (or "managed")
                (
                    None,
                    false,
                    None,
                    if include_values {
                        Some(state_val.clone())
                    } else {
                        None
                    },
                )
            }
        } else {
            // Check for Output changes
            if address.starts_with("output.") {
                let key = address.trim_start_matches("output.");
                if let Some(change) = output_map.get(key) {
                    let mut actions_vec = change.actions.clone();
                    actions_vec.sort();
                    let action_str = actions_vec.join(", ");

                    let values_val = if include_values {
                        process_values(
                            change.after.as_ref(),
                            change.after_unknown.as_ref(),
                            change.after_sensitive.as_ref(),
                        )
                    } else {
                        None
                    };
                    (Some(action_str), false, None, values_val)
                } else {
                    // Output not found in change map. Outputs are handled separately at the top level
                    // of StateValues and are not processed during per-resource recursion.
                    // Treat as no-op if not present in the output change map.
                    (Some("no-op".to_string()), false, None, None)
                }
            } else {
                // If NOT in resource_map and NOT an output...
                let temp_type = determine_block_type(&address, false);
                if temp_type == "data" {
                    // Check if it really exists in the plan state/values, even if no change
                    if active_plan_addresses.contains(&address) {
                        (Some("read".to_string()), true, None, None)
                    } else {
                        // It's a ghost data source from the graph that wasn't evaluated/read.
                        return None;
                    }
                } else if temp_type == "var" || temp_type == "local" || temp_type == "output" {
                    (Some("n/a".to_string()), false, None, None)
                } else {
                    // Strictly filter out resources/data that have no plan entry.
                    return None;
                }
            }
        }
    };

    let node_type = determine_block_type(&address, is_data);

    // Group root variables and outputs
    let final_parent_id = if parent_id.is_none() {
        if node_type == "var" {
            Some("root_variables".to_string())
        } else if node_type == "output" {
            Some("root_outputs".to_string())
        } else {
            None
        }
    } else {
        parent_id
    };

    let final_action = if node_type == "data" && action.is_none() {
        Some("read".to_string())
    } else {
        action
    };

    Some(OutputNode::Resource {
        id: address.clone(),
        parent_id: final_parent_id,
        data: OutputNodeData {
            label: address.clone(),
            node_type,
            action: final_action,
            count,
            values,
            hcl,
        },
        position: OutputNodePosition { x: 0, y: 0 },
    })
}

fn clean_label(label: &str) -> String {
    let s = label.trim();
    let s = if s.starts_with('"') && s.ends_with('"') {
        let inner = &s[1..s.len() - 1];
        inner.replace("\\\"", "\"")
    } else {
        s.to_string()
    };

    // Remove " (expand)", " (expand, reference)", " (close)", etc.
    if let Some(idx) = s.find(" (") {
        s[..idx].to_string()
    } else {
        s
    }
}

fn parse_dot_id(dot_id: &str) -> String {
    // "[root] module.vpc.aws_subnet.public" -> "module.vpc.aws_subnet.public"
    let s = dot_id.trim();
    let s = if let Some(stripped) = s.strip_prefix("[root] ") {
        stripped.to_string()
    } else {
        s.to_string()
    };

    // Also clean suffixes from dot_id if present
    if let Some(idx) = s.find(" (") {
        s[..idx].to_string()
    } else {
        s
    }
}

fn extract_references(val: &serde_json::Value, refs: &mut Vec<String>) {
    match val {
        serde_json::Value::Object(map) => {
            if let Some(serde_json::Value::Array(ref_list)) = map.get("references") {
                for r in ref_list {
                    if let serde_json::Value::String(s) = r {
                        refs.push(s.clone());
                    }
                }
            }
            // Recurse into all values
            for v in map.values() {
                extract_references(v, refs);
            }
        }
        serde_json::Value::Array(arr) => {
            for v in arr {
                extract_references(v, refs);
            }
        }
        _ => {}
    }
}

// Strip attribute paths from resource references
// E.g., "aws_eks_cluster.this[0].endpoint" -> "aws_eks_cluster.this"
// E.g., "module.eks.cluster_endpoint" -> "module.eks.cluster_endpoint"
fn strip_attribute_path(reference: &str) -> String {
    // Split by dots first
    let parts: Vec<&str> = reference.split('.').collect();

    if parts.len() < 2 {
        return reference.to_string();
    }

    // For resource references (type.name.attribute or type.name[0].attribute),
    // keep only type.name (removing bracket notation from the name part)
    // For module/var/local references, keep all parts
    // For data sources (data.type.name.attribute), keep data.type.name
    if parts[0] == "module" || parts[0] == "var" || parts[0] == "local" {
        // Module/var/local references - keep as is
        reference.to_string()
    } else if parts[0] == "data" {
        // Data sources follow pattern: data.type.name[index].attribute
        // Keep data.type.name, removing brackets and attributes
        if parts.len() >= 3 {
            let name_part = parts[2].split('[').next().unwrap_or(parts[2]);
            format!("{}.{}.{}", parts[0], parts[1], name_part)
        } else {
            reference.to_string()
        }
    } else {
        // Resource reference - keep first two parts (type.name), removing brackets from name
        let name_part = parts[1].split('[').next().unwrap_or(parts[1]);
        format!("{}.{}", parts[0], name_part)
    }
}

fn traverse_configuration(
    module: &ModuleConfig,
    parent_path: &str,
    edge_attributes: &mut HashMap<(String, String), HashSet<String>>,
) {
    if let Some(resources) = &module.resources {
        for res in resources {
            // ResourceConfig.address contains only the resource type and name (e.g., "null_resource.foo").
            // For resources in child modules, we must prepend the module path to construct
            // the fully-qualified address (e.g., "module.mod.null_resource.foo").
            let full_address = if parent_path.is_empty() {
                res.address.clone()
            } else {
                format!("{}.{}", parent_path, res.address)
            };

            if let Some(exprs) = &res.expressions {
                for (arg_name, expr_val) in exprs {
                    let mut references = Vec::new();
                    extract_references(expr_val, &mut references);

                    for ref_target in references {
                        // Map reference targets (e.g., "aws_instance.web.id" or "var.region") to dependency node IDs.
                        // Dependencies can be resources ("aws_instance.web"), module outputs ("module.db.output_addr"),
                        // or variables ("var.region").
                        //
                        // Since node IDs may be simplified during graph construction, we store the raw reference
                        // target. Attribute suffixes (e.g., ".id") are preserved as edge matching will handle
                        // fuzzy matching against node ID prefixes.

                        edge_attributes
                            .entry((full_address.clone(), ref_target))
                            .or_default()
                            .insert(arg_name.clone());
                    }
                }
            }

            if let Some(count_expr) = &res.count_expression {
                let mut references = Vec::new();
                extract_references(count_expr, &mut references);
                for ref_target in references {
                    edge_attributes
                        .entry((full_address.clone(), ref_target))
                        .or_default()
                        .insert("count".to_string());
                }
            }

            if let Some(for_each_expr) = &res.for_each_expression {
                let mut references = Vec::new();
                extract_references(for_each_expr, &mut references);
                for ref_target in references {
                    edge_attributes
                        .entry((full_address.clone(), ref_target))
                        .or_default()
                        .insert("for_each".to_string());
                }
            }
        }
    }

    // Also process outputs to capture their dependencies
    if let Some(outputs) = &module.outputs {
        for (name, output_config) in outputs {
            let full_address = if parent_path.is_empty() {
                format!("output.{}", name)
            } else {
                format!("{}.output.{}", parent_path, name)
            };

            if let Some(expr) = &output_config.expression {
                let mut references = Vec::new();
                extract_references(expr, &mut references);

                // Filter out references that are prefixes of other references
                // E.g., if we have ["module.eks.kubernetes_endpoint", "module.eks"],
                // we only want "module.eks.kubernetes_endpoint"
                let filtered_refs: Vec<String> = references
                    .iter()
                    .filter(|ref_a| {
                        // Keep ref_a if no other reference has it as a prefix
                        !references.iter().any(|ref_b| {
                            ref_b.starts_with(&format!("{}.", ref_a)) && ref_b != *ref_a
                        })
                    })
                    .cloned()
                    .collect();

                for ref_target in filtered_refs {
                    // Strip attribute paths from references
                    // E.g., "aws_eks_cluster.this[0].endpoint" -> "aws_eks_cluster.this"
                    let stripped_ref = strip_attribute_path(&ref_target);

                    // Qualify resource references with the module scope
                    // Resources in config use short names (aws_eks_cluster.this) but need
                    // to be qualified with the module path where they're defined
                    // E.g., in module.eks.module.eks, "aws_eks_cluster.this" becomes
                    // "module.eks.module.eks.aws_eks_cluster.this"
                    let qualified_ref = if !parent_path.is_empty()
                        && !stripped_ref.starts_with("module.")
                        && !stripped_ref.starts_with("var.")
                        && !stripped_ref.starts_with("local.")
                        && !stripped_ref.starts_with("output.")
                        && !stripped_ref.starts_with("data.")
                    {
                        // This is a resource reference - qualify it with parent_path
                        format!("{}.{}", parent_path, stripped_ref)
                    } else {
                        // Module/var/local/data references - keep as is
                        stripped_ref
                    };

                    edge_attributes
                        .entry((full_address.clone(), qualified_ref))
                        .or_default()
                        .insert("value".to_string());
                }
            }
        }
    }

    if let Some(calls) = &module.module_calls {
        for (name, call) in calls {
            if let Some(submodule) = &call.module {
                let sub_path = if parent_path.is_empty() {
                    format!("module.{}", name)
                } else {
                    format!("{}.module.{}", parent_path, name)
                };
                traverse_configuration(submodule, &sub_path, edge_attributes);
            }
        }
    }
}

pub fn process_graph(
    plan_json: &str,
    dot_content: &str,
    include_values: bool,
    source_dir: Option<std::path::PathBuf>,
) -> Result<OutputGraph> {
    // 1. Parse Plan File
    let plan: Plan = serde_json::from_str(plan_json).context("Failed to parse plan file")?;

    let index_strip_regex =
        Regex::new(r"\[[^\]]+\]$").context("Failed to compile index strip regex")?;
    let mut resource_map: HashMap<String, Vec<&ResourceChange>> = HashMap::new();
    let output_map: HashMap<String, Change> = plan.output_changes.unwrap_or_default();

    // Collect all known active addresses from Plan (changes + state)
    let mut active_plan_addresses: HashSet<String> = HashSet::new();
    let mut state_values_map: HashMap<String, serde_json::Value> = HashMap::new();

    // Check if we are in "State" mode (direct values with no changes)
    if let Some(values) = &plan.values {
        // Collect everything from root module
        collect_state_addresses(&values.root_module, &mut active_plan_addresses);
        collect_state_values(&values.root_module, &mut state_values_map);
    }

    if let Some(changes) = &plan.resource_changes {
        for change in changes {
            active_plan_addresses.insert(change.address.clone());

            // Index by exact address
            resource_map
                .entry(change.address.clone())
                .or_default()
                .push(change);

            // Index by base address (stripped of trailing index)
            let base = index_strip_regex.replace(&change.address, "").to_string();
            if base != change.address {
                resource_map.entry(base).or_default().push(change);
            }
        }
    }

    if let Some(state) = &plan.prior_state {
        if let Some(values) = &state.values {
            collect_state_addresses(&values.root_module, &mut active_plan_addresses);
        }
    }
    if let Some(state) = &plan.planned_values {
        if let Some(values) = &state.values {
            collect_state_addresses(&values.root_module, &mut active_plan_addresses);
        }
    }

    // 2. Parse DOT File
    let node_regex = Regex::new(r#"^[\t\s]*"(.+?)"\s*\[label\s*=\s*"(.+?)""#)
        .context("Failed to compile node regex")?;
    let edge_regex =
        Regex::new(r#"^[\t\s]*"(.+?)"\s*->\s*"(.+?)""#).context("Failed to compile edge regex")?;

    let mut dot_node_to_address: HashMap<String, String> = HashMap::new();
    let mut known_modules: HashSet<String> = HashSet::new();
    let mut final_nodes: Vec<OutputNode> = Vec::new();
    let mut final_edges: Vec<OutputEdge> = Vec::new();

    let mut raw_nodes: Vec<(String, String)> = Vec::new();
    let mut raw_edges: Vec<(String, String)> = Vec::new();
    let mut file_cache: HashMap<std::path::PathBuf, String> = HashMap::new();

    for line in dot_content.lines() {
        if let Some(caps) = node_regex.captures(line) {
            let dot_id = caps.get(1).unwrap().as_str().to_string();
            let label = caps.get(2).unwrap().as_str().to_string();
            raw_nodes.push((dot_id, label));
        } else if let Some(caps) = edge_regex.captures(line) {
            let source = caps.get(1).unwrap().as_str().to_string();
            let target = caps.get(2).unwrap().as_str().to_string();
            raw_edges.push((source, target));
        }
    }

    // Process Explicit Nodes
    for (dot_id, label) in raw_nodes {
        let address = clean_label(&label);
        let hcl = if let Some(dir) = &source_dir {
            find_hcl_block(dir, &address, &mut file_cache)
        } else {
            None
        };

        if let Some(node) = create_node(
            address.clone(),
            &resource_map,
            &output_map,
            &state_values_map,
            &mut known_modules,
            &mut final_nodes,
            include_values,
            hcl,
            &active_plan_addresses,
        ) {
            dot_node_to_address.insert(dot_id, address);
            final_nodes.push(node);
        }
    }

    // Process Implicit Nodes from Edges
    let mut all_dot_ids = HashSet::new();
    for (s, t) in &raw_edges {
        all_dot_ids.insert(s);
        all_dot_ids.insert(t);
    }

    for dot_id in all_dot_ids {
        if !dot_node_to_address.contains_key(dot_id) {
            let address = parse_dot_id(dot_id);
            let hcl = if let Some(dir) = &source_dir {
                find_hcl_block(dir, &address, &mut file_cache)
            } else {
                None
            };
            if let Some(node) = create_node(
                address.clone(),
                &resource_map,
                &output_map,
                &state_values_map,
                &mut known_modules,
                &mut final_nodes,
                include_values,
                hcl,
                &active_plan_addresses,
            ) {
                dot_node_to_address.insert(dot_id.clone(), address);
                final_nodes.push(node);
            }
        }
    }

    // Reachability Analysis: Prune variables/locals not used by active resources
    {
        let mut adj: HashMap<String, Vec<String>> = HashMap::new();
        for (s, t) in &raw_edges {
            if let (Some(s_addr), Some(t_addr)) =
                (dot_node_to_address.get(s), dot_node_to_address.get(t))
            {
                adj.entry(s_addr.clone()).or_default().push(t_addr.clone());
            }
        }

        let mut seeds = Vec::new();
        for node in &final_nodes {
            if let OutputNode::Resource { id, data, .. } = node {
                // Only treat Active Resources and Root Outputs as seeds.
                // Module outputs, Vars, Locals must be reached to be kept.
                // Data sources are considered active (reads) so they are seeds.
                let is_root_output = data.node_type == "output" && id.starts_with("output.");
                if data.node_type != "var"
                    && data.node_type != "local"
                    && (data.node_type != "output" || is_root_output)
                {
                    seeds.push(id.clone());
                }
            }
        }

        let mut visited = HashSet::new();
        let mut queue = seeds;

        while let Some(u) = queue.pop() {
            if !visited.insert(u.clone()) {
                continue;
            }
            if let Some(neighbors) = adj.get(&u) {
                for v in neighbors {
                    if !visited.contains(v) {
                        queue.push(v.clone());
                    }
                }
            }
        }

        final_nodes.retain(|node| {
            match node {
                OutputNode::Resource { id, data, .. } => {
                    // Note: We don't filter module outputs here because we need them
                    // to build the dependency chain. They'll be filtered later after edges are built.

                    // Prune vars, locals, and data sources if not visited
                    let is_prunable = data.node_type == "var"
                        || data.node_type == "local"
                        || data.node_type == "data";

                    if !is_prunable {
                        true
                    } else {
                        visited.contains(id)
                    }
                }
                OutputNode::Group { .. } => true,
            }
        });
    }

    // Prune invalid/empty module groups
    loop {
        let parent_ids: HashSet<String> = final_nodes
            .iter()
            .flat_map(|n| match n {
                OutputNode::Group { parent_id, .. } => parent_id.clone(),
                OutputNode::Resource { parent_id, .. } => parent_id.clone(),
            })
            .collect();

        let initial_len = final_nodes.len();

        final_nodes.retain(|node| {
            match node {
                OutputNode::Resource { .. } => true, // Always keep resources
                OutputNode::Group { id, .. } => {
                    // Keep group if it is a parent to someone
                    parent_ids.contains(id)
                }
            }
        });

        if final_nodes.len() == initial_len {
            break;
        }
    }

    // Add root_variables group if needed
    let has_root_vars = final_nodes.iter().any(|n| match n {
        OutputNode::Resource {
            parent_id: Some(p), ..
        } => p == "root_variables",
        _ => false,
    });

    if has_root_vars {
        final_nodes.push(OutputNode::Group {
            id: "root_variables".to_string(),
            data: OutputNodeData {
                label: "Variables".to_string(),
                node_type: "group".to_string(),
                action: None,
                count: None,
                values: None,
                hcl: None,
            },
            position: OutputNodePosition { x: 0, y: 0 },
            style: OutputNodeStyle {
                background_color: "rgba(255, 255, 255, 0.05)".to_string(),
                border: "1px dashed #cccccc".to_string(),
                z_index: -1,
            },
            parent_id: None,
        });
    }

    // Add root_outputs group if needed
    let has_root_outs = final_nodes.iter().any(|n| match n {
        OutputNode::Resource {
            parent_id: Some(p), ..
        } => p == "root_outputs",
        _ => false,
    });

    if has_root_outs {
        final_nodes.push(OutputNode::Group {
            id: "root_outputs".to_string(),
            data: OutputNodeData {
                label: "Outputs".to_string(),
                node_type: "group".to_string(),
                action: None,
                count: None,
                values: None,
                hcl: None,
            },
            position: OutputNodePosition { x: 0, y: 0 },
            style: OutputNodeStyle {
                background_color: "rgba(255, 255, 255, 0.05)".to_string(),
                border: "1px dashed #cccccc".to_string(),
                z_index: -1,
            },
            parent_id: None,
        });
    }

    let mut edge_counter = 0;

    // Simplification Phase
    // 1. Build Adjacency List (Dependent -> Dependencies) to traverse 'up'
    // DOT raw_edges are typically: Dependent (Source) -> Dependency (Target).

    let mut deps: HashMap<String, Vec<String>> = HashMap::new();
    let mut node_types: HashMap<String, String> = HashMap::new();

    // Capture types from final_nodes
    for node in &final_nodes {
        match node {
            OutputNode::Resource { id, data, .. } => {
                node_types.insert(id.clone(), data.node_type.clone());
            }
            OutputNode::Group { .. } => {}
        }
    }

    // Build dependency map
    for (s_id, t_id) in &raw_edges {
        if let (Some(s), Some(t)) = (dot_node_to_address.get(s_id), dot_node_to_address.get(t_id)) {
            // Self-loop check
            if s == t {
                continue;
            }
            deps.entry(s.clone()).or_default().push(t.clone());
        }
    }

    // Recursive resolution with caching
    fn resolve_dependencies(
        node: &str,
        deps: &HashMap<String, Vec<String>>,
        types: &HashMap<String, String>,
        cache: &mut HashMap<String, Vec<String>>,
        stack: &mut HashSet<String>,
    ) -> Vec<String> {
        // Break cycles
        if stack.contains(node) {
            return vec![];
        }
        if let Some(cached) = cache.get(node) {
            return cached.clone();
        }

        let n_type_string = types
            .get(node)
            .cloned()
            .unwrap_or_else(|| determine_block_type(node, false));
        let n_type = n_type_string.as_str();

        // We simplify 'var', 'local', and 'output' nodes that act as pass-throughs
        if n_type == "var" || n_type == "local" || n_type == "output" {
            if let Some(my_deps) = deps.get(node) {
                if my_deps.is_empty() {
                    // Root node (no upstream deps found in graph), so it is the source.
                    if n_type == "var" {
                        // Only keep root variables as valid sources without dependencies
                        if !node.starts_with("var.") {
                            return vec![];
                        }
                        return vec![node.to_string()];
                    } else if n_type == "output" {
                        // Module outputs with no deps shouldn't appear - traverse through them
                        if node.starts_with("output.") {
                            // Root outputs are sources
                            return vec![node.to_string()];
                        } else {
                            // Module outputs: should have been resolved away, return empty
                            return vec![];
                        }
                    } else {
                        // locals that resolve to nothing are hidden (constants)
                        return vec![];
                    }
                }

                // It has dependencies, so it is likely an intermediate.
                // Recurse up.
                stack.insert(node.to_string());
                let mut resolved = Vec::new();
                for d in my_deps {
                    resolved.extend(resolve_dependencies(d, deps, types, cache, stack));
                }
                stack.remove(node);

                // For module outputs: return the resolved dependencies (traverse through)
                // For root outputs: if no resolved deps, return self
                if n_type == "output" && !node.starts_with("output.") {
                    // Module output: traverse through it, don't include it in results
                    resolved.sort();
                    resolved.dedup();
                    cache.insert(node.to_string(), resolved.clone());
                    return resolved;
                }

                if resolved.is_empty() {
                    if n_type == "var" {
                        if !node.starts_with("var.") {
                            return vec![];
                        }
                        return vec![node.to_string()];
                    } else if n_type == "output" {
                        // Root output that couldn't resolve to anything
                        if node.starts_with("output.") {
                            return vec![node.to_string()];
                        } else {
                            return vec![];
                        }
                    } else {
                        return vec![];
                    }
                }

                resolved.sort();
                resolved.dedup();
                cache.insert(node.to_string(), resolved.clone());
                return resolved;
            } else {
                // No dependencies recorded in the graph
                if n_type == "var" {
                    if !node.starts_with("var.") {
                        return vec![];
                    }
                    return vec![node.to_string()];
                } else if n_type == "output" {
                    // Only keep root outputs as sources
                    // Module outputs without deps should be hidden
                    if node.starts_with("output.") {
                        return vec![node.to_string()];
                    } else {
                        return vec![];
                    }
                } else {
                    return vec![];
                }
            }
        }

        // Not a simplified type, return self
        vec![node.to_string()]
    }

    // Process configuration to extract output dependencies BEFORE building simplified_edges
    let mut config_deps: HashMap<(String, String), HashSet<String>> = HashMap::new();
    if let Some(config) = &plan.configuration {
        traverse_configuration(&config.root_module, "", &mut config_deps);
    }

    // Augment deps map with output dependencies from configuration
    // (DOT graph may not include these edges)
    for ((dependent, ref_target), _) in &config_deps {
        // Only add if dependent is an output
        if dependent.starts_with("output.") || dependent.contains(".output.") {
            // Extract the scope of the dependent to qualify the reference
            // E.g., "module.eks.output.kubernetes_endpoint" -> scope is "module.eks"
            let dependent_scope = if let Some(output_pos) = dependent.rfind(".output.") {
                &dependent[..output_pos]
            } else {
                "" // Root level output
            };

            // Qualify the ref_target with the dependent's scope and normalize it
            // E.g., if dependent is "module.eks.output.kubernetes_endpoint" (scope: "module.eks")
            //   and ref_target is "module.eks.cluster_endpoint"
            //   then qualified target is "module.eks.module.eks.cluster_endpoint"
            //   which normalizes to "module.eks.module.eks.output.cluster_endpoint"
            //
            // BUT: If ref_target already starts with the dependent_scope, don't qualify it again
            // E.g., if dependent is "module.eks.module.eks.output.cluster_endpoint" (scope: "module.eks.module.eks")
            //   and ref_target is "module.eks.module.eks.aws_eks_cluster.this"
            //   then it already has the full path, so don't add scope again
            let qualified_target =
                if !dependent_scope.is_empty() && ref_target.starts_with("module.") {
                    if ref_target.starts_with(&format!("{}.", dependent_scope))
                        || ref_target == dependent_scope
                    {
                        // Already qualified with the correct scope
                        ref_target.clone()
                    } else {
                        // Need to qualify
                        format!("{}.{}", dependent_scope, ref_target)
                    }
                } else {
                    ref_target.clone()
                };

            // Transform module output references: "module.X.output_name" -> "module.X.output.output_name"
            // But ONLY if it's not already a full resource path (aws_*, data.*, etc.)
            let normalized_target = if qualified_target.starts_with("module.") {
                let parts: Vec<&str> = qualified_target.split('.').collect();

                // Determine whether this is a resource reference or a module output reference
                // by counting the non-module-chain parts at the end of the path.
                //
                // After consuming all leading "module.X" pairs, the remaining parts are:
                //   1 part  → output reference (e.g. "module.inner.inner_out")
                //   2 parts → resource reference (e.g. "module.eks.aws_eks_cluster.this")
                //
                // This is provider-agnostic and handles arbitrary nesting:
                //   module.a.module.b.cluster_endpoint   → 1 remaining → output
                //   module.a.module.b.aws_eks_cluster.x  → 2 remaining → resource
                let mut i = 0;
                while i + 1 < parts.len() && parts[i] == "module" {
                    i += 2;
                }
                let remaining = parts.len() - i;
                let is_resource_ref = remaining >= 2;

                if !is_resource_ref && !parts.contains(&"output") {
                    // This is a module output reference - insert "output" before the last part
                    let mut new_parts = parts[..parts.len() - 1].to_vec();
                    new_parts.push("output");
                    new_parts.push(parts[parts.len() - 1]);
                    new_parts.join(".")
                } else {
                    qualified_target.clone()
                }
            } else {
                qualified_target.clone()
            };

            deps.entry(dependent.clone())
                .or_default()
                .push(normalized_target);
        }
    }

    let mut cache = HashMap::new();
    let mut stack = HashSet::new();
    let mut simplified_edges: Vec<(String, String, String)> = Vec::new();
    let mut active_nodes: HashSet<String> = HashSet::new();

    // Re-calculate edges for all nodes
    for (node, n_type) in &node_types {
        // We do NOT want to simplify root outputs (e.g. "output.foo"), because they are 'sinks'
        // that we want to visualize. Module outputs are intermediates.
        let is_root_output = node.starts_with("output.");

        let is_simplifiable =
            (n_type == "var" || n_type == "local" || n_type == "output") && !is_root_output;
        let has_deps = deps.contains_key(node) && !deps[node].is_empty();

        if is_simplifiable {
            if has_deps {
                continue;
            }
            // If it is a constant local (no deps), we hide it.
            // Vars and constant Outputs are kept as sources, BUT only if Vars are root variables.
            if n_type == "local" {
                continue;
            }
            if n_type == "var" && !node.starts_with("var.") {
                continue;
            }
        }

        // Keep non-intermediate nodes (Resources, Data, Root Vars)
        active_nodes.insert(node.clone());

        // Now resolve immediate dependencies to find the ultimate source
        if let Some(my_deps) = deps.get(node) {
            for dep in my_deps {
                let resolved_sources =
                    resolve_dependencies(dep, &deps, &node_types, &mut cache, &mut stack);
                for source in resolved_sources {
                    if source != *node {
                        simplified_edges.push((node.clone(), source.clone(), dep.clone())); // Dependent, Dependency, Via
                        active_nodes.insert(source);
                    }
                }
            }
        }
    }

    // Optimize lookup: Map Dependent -> List of (Reference, AttributeNames)
    let mut config_lookup: HashMap<String, Vec<(String, HashSet<String>)>> = HashMap::new();
    for ((dep, ref_t), args) in config_deps {
        config_lookup.entry(dep).or_default().push((ref_t, args));
    }

    let mut current_edges: HashMap<(String, String), HashSet<String>> = HashMap::new();

    // Now create final_edges from simplified_edges, struct wants: source: Dependency, target: Dependent
    for (dependent, dependency, via_node) in simplified_edges {
        let mut attributes = Vec::new();

        if let Some(refs) = config_lookup.get(&dependent) {
            // Determine scope of dependent for qualification
            let dependent_scope = {
                let parts: Vec<&str> = dependent.split('.').collect();
                let mut scope = Vec::new();
                let mut i = 0;
                while i < parts.len() {
                    if parts[i] == "module" && i + 1 < parts.len() {
                        scope.push("module");
                        scope.push(parts[i + 1]);
                        i += 2;
                    } else {
                        break;
                    }
                }
                scope.join(".")
            };

            for (ref_target, arg_names) in refs {
                // Qualify the reference found in configuration with the scope of the dependent
                let qualified_ref = if dependent_scope.is_empty() {
                    ref_target.clone()
                } else {
                    format!("{}.{}", dependent_scope, ref_target)
                };

                // Check for match between via_node (Immediate Dependency in Graph) and qualified_ref
                let matches = if via_node == qualified_ref {
                    true
                } else if qualified_ref.starts_with(&via_node) {
                    // Ref is child of Via (e.g. via="mod.var", ref="mod.var[0]")
                    let r = &qualified_ref[via_node.len()..];
                    r.starts_with('.') || r.starts_with('[')
                } else if via_node.starts_with(&qualified_ref) {
                    // Via is child of Ref (e.g. Ref="mod.var", Via="mod.var[0]")
                    let r = &via_node[qualified_ref.len()..];
                    r.starts_with('.') || r.starts_with('[')
                } else {
                    false
                };

                if matches {
                    attributes.extend(arg_names.clone());
                }
            }
        }

        current_edges
            .entry((dependency, dependent))
            .or_default()
            .extend(attributes);
    }

    // Collect all nodes referenced in edges before creating edges
    let mut nodes_in_edges: HashSet<String> = HashSet::new();
    for ((source, target), _) in &current_edges {
        nodes_in_edges.insert(source.clone());
        nodes_in_edges.insert(target.clone());
    }

    for ((source, target), attributes_set) in current_edges {
        edge_counter += 1;
        let mut attributes: Vec<String> = attributes_set.into_iter().collect();
        attributes.sort();

        let attributes_opt = if attributes.is_empty() {
            None
        } else {
            Some(attributes)
        };

        final_edges.push(OutputEdge {
            id: format!("e_{}", edge_counter),
            source,
            target,
            attributes: attributes_opt,
        });
    }

    let mut filtered_nodes = Vec::new();

    for node in final_nodes {
        match node {
            OutputNode::Resource {
                ref id, ref data, ..
            } => {
                // Always hide module outputs (non-root outputs) - they should be simplified away
                if data.node_type == "output" && !id.starts_with("output.") {
                    continue;
                }

                // Keep node if it's in active_nodes OR if it's referenced by any edge
                if active_nodes.contains(id) || nodes_in_edges.contains(id) {
                    filtered_nodes.push(node);
                }
            }
            OutputNode::Group { .. } => {
                filtered_nodes.push(node);
            }
        }
    }
    final_nodes = filtered_nodes;

    // Remove empty module groups (groups with no children)
    loop {
        let parent_ids: HashSet<String> = final_nodes
            .iter()
            .flat_map(|n| match n {
                OutputNode::Group { parent_id, .. } => parent_id.clone(),
                OutputNode::Resource { parent_id, .. } => parent_id.clone(),
            })
            .collect();

        let initial_len = final_nodes.len();

        final_nodes.retain(|node| {
            match node {
                OutputNode::Resource { .. } => true,
                OutputNode::Group { id, .. } => {
                    // Keep group if it has children
                    parent_ids.contains(id)
                }
            }
        });

        if final_nodes.len() == initial_len {
            break;
        }
    }

    let active_node_ids: HashSet<String> = final_nodes
        .iter()
        .filter_map(|n| match n {
            OutputNode::Resource { id, .. } => Some(id.clone()),
            _ => None,
        })
        .collect();

    let active_parent_ids: HashSet<String> = final_nodes
        .iter()
        .filter_map(|n| match n {
            OutputNode::Resource { parent_id, .. } => parent_id.clone(),
            OutputNode::Group { parent_id, .. } => parent_id.clone(),
        })
        .collect();

    final_nodes.retain(|n| match n {
        OutputNode::Resource { .. } => true,
        OutputNode::Group { id, .. } => {
            active_node_ids
                .iter()
                .any(|nid| nid.starts_with(&format!("{}.", id)))
                || active_nodes.contains(id)
                || active_parent_ids.contains(id)
        }
    });

    let mut unique_nodes = HashMap::new();
    for node in final_nodes {
        let id = match &node {
            OutputNode::Group { id, .. } => id.clone(),
            OutputNode::Resource { id, .. } => id.clone(),
        };
        unique_nodes.entry(id).or_insert(node);
    }

    let output_nodes: Vec<OutputNode> = unique_nodes.into_values().collect();

    Ok(OutputGraph {
        nodes: output_nodes,
        edges: final_edges,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_multiple_indices() {
        let plan_json = r#"{
            "resource_changes": [
                {
                    "address": "module.vpc.aws_subnet.public[0]",
                    "type": "aws_subnet",
                    "change": { "actions": ["create"] }
                },
                {
                    "address": "module.vpc.aws_subnet.public[1]",
                    "type": "aws_subnet",
                    "change": { "actions": ["delete"] }
                }
            ]
        }"#;

        // Graph dot has the base name
        let dot_content = r#"
            digraph {
                "[root] module.vpc.aws_subnet.public" [label = "module.vpc.aws_subnet.public"]
            }
        "#;

        let graph = process_graph(plan_json, dot_content, false, None).unwrap();
        let node = graph
            .nodes
            .iter()
            .find(|n| match n {
                OutputNode::Resource { id, .. } => id == "module.vpc.aws_subnet.public",
                _ => false,
            })
            .expect("Node should exist");

        match node {
            OutputNode::Resource { data, .. } => {
                // Actions should be aggregated and sorted
                assert_eq!(data.action, Some("create, delete".to_string()));
            }
            _ => panic!("Wrong node type"),
        }
    }

    #[test]
    fn test_action_resolution() {
        let plan_json = r#"{
            "resource_changes": [
                {
                    "address": "data.aws_ami.ubuntu",
                    "type": "aws_ami",
                    "mode": "data",
                    "change": { "actions": ["read"] }
                },
                {
                    "address": "aws_instance.foo",
                    "type": "aws_instance",
                    "change": { "actions": ["no-op"] }
                },
                {
                    "address": "aws_instance.bar",
                    "type": "aws_instance",
                    "change": { "actions": ["update"] }
                }
            ]
        }"#;

        let dot_content = r#"
            digraph {
                "[root] data.aws_ami.ubuntu" [label = "data.aws_ami.ubuntu"]
                "[root] aws_instance.foo" [label = "aws_instance.foo"]
                "[root] aws_instance.bar" [label = "aws_instance.bar"]
                "[root] aws_instance.baz" [label = "aws_instance.baz"]
                "[root] aws_instance.bar" -> "[root] data.aws_ami.ubuntu"
            }
        "#;

        let graph = process_graph(plan_json, dot_content, false, None).unwrap();

        // 1. Data Source
        let data_node = graph
            .nodes
            .iter()
            .find(|n| match n {
                OutputNode::Resource { id, .. } => id == "data.aws_ami.ubuntu",
                _ => false,
            })
            .unwrap();
        match data_node {
            OutputNode::Resource { data, .. } => {
                assert_eq!(data.node_type, "data");
                assert_eq!(data.action, Some("read".to_string()));
            }
            _ => panic!(),
        };

        // 2. Explicit No-Op
        let foo_node = graph
            .nodes
            .iter()
            .find(|n| match n {
                OutputNode::Resource { id, .. } => id == "aws_instance.foo",
                _ => false,
            })
            .unwrap();
        match foo_node {
            OutputNode::Resource { data, .. } => {
                assert_eq!(data.node_type, "resource");
                // "no-op" is preserved if it is the only action
                assert_eq!(data.action, Some("no-op".to_string()));
            }
            _ => panic!(),
        };

        // 3. Implicit No-Op (Missing from plan) - PRUNED
        let baz_node = graph.nodes.iter().find(|n| match n {
            OutputNode::Resource { id, .. } => id == "aws_instance.baz",
            _ => false,
        });
        // With pruning enabled, resources missing from the plan should be removed
        assert!(baz_node.is_none());
    }

    #[test]
    fn test_include_values() {
        let plan_json = r#"{
            "resource_changes": [
                {
                    "address": "aws_instance.foo",
                    "type": "aws_instance",
                    "change": { 
                        "actions": ["create"],
                        "after": { "ami": "ami-123456", "instance_type": "t3.micro" }
                    }
                }
            ]
        }"#;

        let dot_content = r#"
            digraph {
                "[root] aws_instance.foo" [label = "aws_instance.foo"]
            }
        "#;

        // Check with include_values = true
        let graph = process_graph(plan_json, dot_content, true, None).unwrap();
        let node = graph
            .nodes
            .iter()
            .find(|n| match n {
                OutputNode::Resource { id, .. } => id == "aws_instance.foo",
                _ => false,
            })
            .unwrap();

        match node {
            OutputNode::Resource { data, .. } => {
                let values = data.values.as_ref().expect("values should be present");
                assert_eq!(values["ami"], "ami-123456");
                assert_eq!(values["instance_type"], "t3.micro");
            }
            _ => panic!("Wrong node type"),
        }

        // Check with include_values = false
        let graph = process_graph(plan_json, dot_content, false, None).unwrap();
        let node = graph
            .nodes
            .iter()
            .find(|n| match n {
                OutputNode::Resource { id, .. } => id == "aws_instance.foo",
                _ => false,
            })
            .unwrap();

        match node {
            OutputNode::Resource { data, .. } => {
                assert!(data.values.is_none());
            }
            _ => panic!("Wrong node type"),
        }
    }

    #[test]
    fn test_count_expression_attribute_extraction() {
        // Setup: A resource with a count expression depending on "var.azs"
        let resource_name = "aws_route_table.intra";
        let dependency = "var.azs";

        let resource_config = ResourceConfig {
            address: resource_name.to_string(),
            expressions: None,
            count_expression: Some(json!({
                "references": [dependency]
            })),
            for_each_expression: None,
        };

        let module_config = ModuleConfig {
            resources: Some(vec![resource_config]),
            module_calls: None,
            outputs: None,
        };

        let mut edge_attributes = HashMap::new();

        // Execute
        traverse_configuration(&module_config, "", &mut edge_attributes);

        // Verify
        let key = (resource_name.to_string(), dependency.to_string());

        assert!(edge_attributes.contains_key(&key), "Edge should exist");
        let attributes = edge_attributes.get(&key).unwrap();
        assert!(
            attributes.contains("count"),
            "Attribute 'count' should be present"
        );

        println!("Found matching edge attributes: {:?}", attributes);
    }

    #[test]
    fn test_for_each_expression_attribute_extraction() {
        // Setup: A resource with a for_each expression depending on "var.maps"
        let resource_name = "aws_instance.server";
        let dependency = "var.maps";

        let resource_config = ResourceConfig {
            address: resource_name.to_string(),
            expressions: None,
            count_expression: None,
            for_each_expression: Some(json!({
                "references": [dependency]
            })),
        };

        let module_config = ModuleConfig {
            resources: Some(vec![resource_config]),
            module_calls: None,
            outputs: None,
        };

        let mut edge_attributes = HashMap::new();

        // Execute
        traverse_configuration(&module_config, "", &mut edge_attributes);

        // Verify
        let key = (resource_name.to_string(), dependency.to_string());

        assert!(edge_attributes.contains_key(&key), "Edge should exist");
        let attributes = edge_attributes.get(&key).unwrap();
        assert!(
            attributes.contains("for_each"),
            "Attribute 'for_each' should be present"
        );
    }

    #[test]
    fn test_value_merging() {
        let after = json!({
            "known": "value",
            "secret": "hidden",
            // "missing_in_after": true  <-- implicitly missing
            "nested": {
                "a": 1,
                "b": "secret"
            },
            "list": ["a", "secret_element", null]
        });

        let unknown = json!({
            "unknown": true,
            "missing_in_after": true,
            "list": [false, false, true]
        });

        let sensitive = json!({
            "secret": true,
            "nested": {
                "b": true
            },
             "list": [false, true, false]
        });

        let processed = process_values(Some(&after), Some(&unknown), Some(&sensitive)).unwrap();

        assert_eq!(processed["known"], "value");
        assert_eq!(processed["secret"], "(sensitive)");
        assert_eq!(processed["unknown"], "(known after apply)");
        assert_eq!(processed["missing_in_after"], "(known after apply)"); // New check

        assert_eq!(processed["nested"]["a"], 1);
        assert_eq!(processed["nested"]["b"], "(sensitive)");

        let list = processed["list"].as_array().unwrap();
        assert_eq!(list[0], "a");
        assert_eq!(list[1], "(sensitive)");
        assert_eq!(list[2], "(known after apply)");
    }
}

#[derive(Deserialize, Debug)]
struct ModulesJson {
    #[serde(rename = "Modules")]
    modules: Vec<ModuleEntry>,
}

#[derive(Deserialize, Debug)]
struct ModuleEntry {
    #[serde(rename = "Key")]
    key: String,
    #[serde(rename = "Dir")]
    dir: String,
}

fn get_module_dir(root_dir: &std::path::Path, address: &str) -> Option<std::path::PathBuf> {
    let modules_json_path = root_dir.join(".terraform/modules/modules.json");
    if let Ok(content) = std::fs::read_to_string(&modules_json_path) {
        if let Ok(modules_data) = serde_json::from_str::<ModulesJson>(&content) {
            let parts: Vec<&str> = address.split('.').collect();
            let mut key_parts = Vec::new();
            let mut i = 0;
            while i < parts.len() {
                if parts[i] == "module" && i + 1 < parts.len() {
                    key_parts.push(parts[i + 1]);
                    i += 2;
                } else {
                    break;
                }
            }
            let key = key_parts.join(".");
            for m in modules_data.modules {
                if m.key == key {
                    return Some(root_dir.join(m.dir));
                }
            }
        }
    }

    // Fallback/Root
    if !address.contains("module.") {
        Some(root_dir.to_path_buf())
    } else {
        None
    }
}

fn extract_block_from_content(
    content: &str,
    node_type: &str,
    resource_type: &str,
    name: &str,
) -> Option<String> {
    let pattern = if node_type == "module" {
        format!(r#"(?m)^[\t ]*module\s+"{}"\s*\{{"#, regex::escape(name))
    } else {
        format!(
            r#"(?m)^[\t ]*{}\s+"{}"\s+"{}"\s*\{{"#,
            node_type,
            regex::escape(resource_type),
            regex::escape(name)
        )
    };

    let re = Regex::new(&pattern).ok()?;

    if let Some(mat) = re.find(content) {
        let start = mat.start();
        let mut brace_count = 0;
        let mut in_string = false;
        let mut escape = false;
        let mut found_start = false;
        let mut end = start;

        for (i, c) in content[start..].char_indices() {
            if !found_start {
                if c == '{' {
                    found_start = true;
                    brace_count = 1;
                }
                continue;
            }

            if c == '"' && !escape {
                in_string = !in_string;
            }

            if !in_string {
                if c == '{' {
                    brace_count += 1;
                } else if c == '}' {
                    brace_count -= 1;
                }
            }

            if c == '\\' {
                escape = !escape;
            } else {
                escape = false;
            }

            if brace_count == 0 {
                end = start + i + 1;
                break;
            }
        }

        if end > start {
            // Un-escape the string? No, keep it raw as user requested.
            // But verify it looks clean.
            return Some(content[start..end].to_string());
        }
    }
    None
}

fn find_hcl_block(
    root_dir: &std::path::Path,
    address: &str,
    cache: &mut HashMap<std::path::PathBuf, String>,
) -> Option<String> {
    let module_dir = get_module_dir(root_dir, address)?;

    let parts: Vec<&str> = address.split('.').collect();
    let mut local_parts = Vec::new();
    let mut i = 0;
    while i < parts.len() {
        if parts[i] == "module" && i + 1 < parts.len() {
            i += 2;
        } else {
            local_parts.push(parts[i]);
            i += 1;
        }
    }

    if local_parts.is_empty() {
        return None;
    }

    let (node_type, res_type, name) = if local_parts[0] == "data" {
        if local_parts.len() < 3 {
            return None;
        }
        ("data", local_parts[1], local_parts[2])
    } else if local_parts[0] == "var" || local_parts[0] == "output" {
        return None;
    } else {
        if local_parts.len() == 2 {
            ("resource", local_parts[0], local_parts[1])
        } else {
            return None;
        }
    };

    if let Ok(entries) = std::fs::read_dir(&module_dir) {
        for entry in entries {
            if let Ok(entry) = entry {
                let path = entry.path();
                if path.extension().and_then(|s| s.to_str()) == Some("tf") {
                    let content = if let Some(c) = cache.get(&path) {
                        c.clone()
                    } else {
                        let c = std::fs::read_to_string(&path).unwrap_or_default();
                        cache.insert(path.clone(), c.clone());
                        c
                    };

                    if let Some(block) =
                        extract_block_from_content(&content, node_type, res_type, name)
                    {
                        return Some(block);
                    }
                }
            }
        }
    }
    None
}

fn collect_state_addresses(module: &StateModule, addresses: &mut HashSet<String>) {
    if let Some(resources) = &module.resources {
        for res in resources {
            addresses.insert(res.address.clone());
        }
    }
    if let Some(children) = &module.child_modules {
        for child in children {
            collect_state_addresses(child, addresses);
        }
    }
}

fn collect_state_values(module: &StateModule, values_map: &mut HashMap<String, serde_json::Value>) {
    if let Some(resources) = &module.resources {
        for res in resources {
            if let Some(val) = &res.values {
                values_map.insert(res.address.clone(), val.clone());
            }
        }
    }
    if let Some(children) = &module.child_modules {
        for child in children {
            collect_state_values(child, values_map);
        }
    }
}
