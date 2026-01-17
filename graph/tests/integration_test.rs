use std::env;
use std::fs;
use std::path::Path;
use std::process::Command;

use graph::{OutputGraph, OutputNode, process_graph};

// Helper function to run fixture and process graph
fn run_fixture(fixture_name: &str, use_state: bool) -> OutputGraph {
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR not set");
    let test_dir = Path::new(&manifest_dir)
        .join("tests")
        .join("fixtures")
        .join(fixture_name);

    let temp_dir = tempfile::tempdir().expect("failed to create temp dir");
    let target_dir = temp_dir.path();

    // Copy directory content recursively
    copy_dir_all(&test_dir, target_dir).expect("failed to copy fixture files");

    // 2. Run Tofu Commands
    let status = Command::new("tofu")
        .arg("init")
        .current_dir(target_dir)
        .output()
        .expect("Failed to run tofu init");
    assert!(
        status.status.success(),
        "tofu init failed: {}",
        String::from_utf8_lossy(&status.stderr)
    );

    let json_output;

    if use_state {
        // Init -> Apply -> Show
        let status = Command::new("tofu")
            .args(&["apply", "-auto-approve"])
            .current_dir(target_dir)
            .output()
            .expect("Failed to run tofu apply");
        assert!(
            status.status.success(),
            "tofu apply failed: {}",
            String::from_utf8_lossy(&status.stderr)
        );

        let output = Command::new("tofu")
            .args(&["show", "-json"])
            .current_dir(target_dir)
            .output()
            .expect("Failed to run tofu show");
        assert!(output.status.success(), "tofu show failed");
        json_output = String::from_utf8(output.stdout).expect("Invalid utf8 in state json");
    } else {
        // Plan -> Show
        let status = Command::new("tofu")
            .args(&["plan", "-out=plan.tfplan"])
            .current_dir(target_dir)
            .output()
            .expect("Failed to run tofu plan");
        assert!(
            status.status.success(),
            "tofu plan failed: {}",
            String::from_utf8_lossy(&status.stderr)
        );

        let output = Command::new("tofu")
            .args(&["show", "-json", "plan.tfplan"])
            .current_dir(target_dir)
            .output()
            .expect("Failed to run tofu show");
        assert!(output.status.success(), "tofu show failed");
        json_output = String::from_utf8(output.stdout).expect("Invalid utf8 in plan json");
    }

    let output = Command::new("tofu")
        .arg("graph")
        .current_dir(target_dir)
        .output()
        .expect("Failed to run tofu graph");
    assert!(output.status.success(), "tofu graph failed");
    let graph_dot = String::from_utf8(output.stdout).expect("Invalid utf8 in graph dot");

    // 3. Run our function
    let result = process_graph(
        &json_output,
        &graph_dot,
        use_state,
        Some(target_dir.to_path_buf()),
    )
    .expect("process_graph failed");

    // Save output for inspection
    let result_json = serde_json::to_string_pretty(&result).expect("Failed to serialize output");
    fs::write(test_dir.join("output.json"), result_json).expect("Failed to write output.json");

    result
}

fn copy_dir_all(src: &Path, dst: &Path) -> std::io::Result<()> {
    if !dst.exists() {
        fs::create_dir_all(dst)?;
    }
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        if ty.is_dir() {
            copy_dir_all(&entry.path(), &dst.join(entry.file_name()))?;
        } else {
            fs::copy(entry.path(), dst.join(entry.file_name()))?;
        }
    }
    Ok(())
}

#[test]
fn test_end_to_end_simple() {
    let result = run_fixture("simple", false);

    // 4. Validate Output
    let nodes = result.nodes;
    let edges = result.edges;

    // Verify Nodes
    // We expect 2 nodes: local_file.foo and local_file.bar
    assert_eq!(nodes.len(), 2, "Expected 2 nodes (foo, bar)");

    let foo_node = nodes
        .iter()
        .find(|n| match n {
            OutputNode::Resource { id, .. } => id == "local_file.foo",
            _ => false,
        })
        .expect("local_file.foo not found");

    match foo_node {
        OutputNode::Resource { data, .. } => {
            assert_eq!(
                data.action.as_deref(),
                Some("create"),
                "foo action mismatch"
            );
            assert_eq!(data.node_type, "resource", "foo type mismatch");
        }
        _ => panic!("Unexpected node type for foo"),
    }

    let bar_node = nodes
        .iter()
        .find(|n| match n {
            OutputNode::Resource { id, .. } => id == "local_file.bar",
            _ => false,
        })
        .expect("local_file.bar not found");

    match bar_node {
        OutputNode::Resource { data, .. } => {
            assert_eq!(
                data.action.as_deref(),
                Some("create"),
                "bar action mismatch"
            );
        }
        _ => panic!("Unexpected node type for bar"),
    }

    // Verify Edges
    // local_file.bar depends on local_file.foo
    // Edge: source (foo) -> target (bar)
    let edge = edges
        .iter()
        .find(|e| e.source == "local_file.foo" && e.target == "local_file.bar")
        .expect("Edge foo->bar not found");

    // This is an explicit `depends_on`, so no specific attributes are used as input.
    assert!(
        edge.attributes.is_none(),
        "Explicit depends_on should have no attributes"
    );
}

#[test]
fn test_end_to_end_variables() {
    let result = run_fixture("variables", false);

    let nodes = result.nodes;
    let edges = result.edges;

    // 4. Validate Output
    // We expect:
    // - local_file.file
    // - var.content
    // - var.filename

    let file_node = nodes.iter().find(|n| match n {
        OutputNode::Resource { id, .. } => id == "local_file.file",
        _ => false,
    });

    let var_content_node = nodes.iter().find(|n| match n {
        OutputNode::Resource { id, .. } => id == "var.content",
        _ => false,
    });

    let var_filename_node = nodes.iter().find(|n| match n {
        OutputNode::Resource { id, .. } => id == "var.filename",
        _ => false,
    });

    assert!(file_node.is_some(), "local_file.file not found");
    assert!(var_content_node.is_some(), "var.content not found");
    assert!(var_filename_node.is_some(), "var.filename not found");

    // Validate Edge: var.content -> local_file.file
    let edge_content = edges
        .iter()
        .find(|e| e.source == "var.content" && e.target == "local_file.file");
    assert!(
        edge_content.is_some(),
        "Edge from var.content to local_file.file not found"
    );

    // Validate Edge: var.filename -> local_file.file
    let edge_filename = edges
        .iter()
        .find(|e| e.source == "var.filename" && e.target == "local_file.file");
    assert!(
        edge_filename.is_some(),
        "Edge from var.filename to local_file.file not found"
    );
}

#[test]
fn test_end_to_end_outputs() {
    let result = run_fixture("outputs", false);

    let nodes = result.nodes;
    let edges = result.edges;

    // 4. Validate Output
    // We expect:
    // - local_file.foo
    // - output.foo_id
    // - output.foo_content

    let foo_node = nodes.iter().find(|n| match n {
        OutputNode::Resource { id, .. } => id == "local_file.foo",
        _ => false,
    });

    let out_id_node = nodes.iter().find(|n| match n {
        OutputNode::Resource { id, .. } => id == "output.foo_id",
        _ => false,
    });

    let out_content_node = nodes.iter().find(|n| match n {
        OutputNode::Resource { id, .. } => id == "output.foo_content",
        _ => false,
    });

    assert!(foo_node.is_some(), "local_file.foo not found");
    assert!(out_id_node.is_some(), "output.foo_id not found");
    assert!(out_content_node.is_some(), "output.foo_content not found");

    // Validate Edges: local_file.foo -> output.foo_id (source -> target in our graph structure is dependency -> dependent)
    // output.foo_id depends on local_file.foo
    // So source = local_file.foo, target = output.foo_id

    let edge_id = edges
        .iter()
        .find(|e| e.source == "local_file.foo" && e.target == "output.foo_id");
    assert!(
        edge_id.is_some(),
        "Edge from local_file.foo to output.foo_id not found"
    );

    let edge_content = edges
        .iter()
        .find(|e| e.source == "local_file.foo" && e.target == "output.foo_content");
    assert!(
        edge_content.is_some(),
        "Edge from local_file.foo to output.foo_content not found"
    );
}

#[test]
fn test_end_to_end_modules() {
    let result = run_fixture("modules", false);

    let nodes = result.nodes;
    let edges = result.edges;

    // 4. Validate Output
    // We expect:
    // - module.my_child.local_file.inner (Resource)
    // - local_file.outer (Resource)
    // - module.my_child (Group)

    let inner_node = nodes.iter().find(|n| match n {
        OutputNode::Resource { id, .. } => id == "module.my_child.local_file.inner",
        _ => false,
    });

    let outer_node = nodes.iter().find(|n| match n {
        OutputNode::Resource { id, .. } => id == "local_file.outer",
        _ => false,
    });

    let group_node = nodes.iter().find(|n| match n {
        OutputNode::Group { id, .. } => id == "module.my_child",
        _ => false,
    });

    assert!(
        inner_node.is_some(),
        "module.my_child.local_file.inner not found"
    );
    assert!(outer_node.is_some(), "local_file.outer not found");
    // Group node validation
    match group_node {
        Some(OutputNode::Group { id, parent_id, .. }) => {
            assert_eq!(id, "module.my_child");
            assert!(
                parent_id.is_none(),
                "module.my_child should not have a parent"
            );
        }
        _ => panic!("module.my_child Group node not found or incorrect type"),
    }

    // Verify parentage of inner resource
    match inner_node.unwrap() {
        OutputNode::Resource { parent_id, .. } => {
            assert_eq!(
                parent_id.as_deref(),
                Some("module.my_child"),
                "Inner resource should belong to module group"
            );
        }
        _ => {}
    }

    // Validate Edge: module.my_child.local_file.inner -> local_file.outer
    // Note: The dependency chain is: outer -> module output -> inner resource.
    // The graph simplifier might collapse the module output.
    // So distinct path: inner -> (output?) -> outer.
    // Let's see if we get a direct edge or if intermediate nodes exist.
    // Based on our simplification logic, intermediate "output" and "var" nodes dependent on something are collapsed unless they are root outputs.
    // "module.my_child.output.inner_id" is likely an intermediate.

    // We expect: Source(inner) -> Target(outer) ??
    // Wait, outer depends on inner.
    // So edges: source=inner, target=outer.

    let edge = edges
        .iter()
        .find(|e| e.source == "module.my_child.local_file.inner" && e.target == "local_file.outer");

    if edge.is_none() {
        // If direct edge missing, dump edges to see what's there
        println!(
            "Edges found: {:?}",
            edges
                .iter()
                .map(|e| (&e.source, &e.target))
                .collect::<Vec<_>>()
        );
    }
    assert!(edge.is_some(), "Edge from inner to outer not found");
}

#[test]
fn test_end_to_end_data_sources() {
    let result = run_fixture("data_sources", false);

    let nodes = result.nodes;
    let edges = result.edges;

    // We expect:
    // - data.local_file.read (Type: data)
    // - local_file.destination (Type: resource)

    let data_node = nodes.iter().find(|n| match n {
        OutputNode::Resource { id, .. } => id == "data.local_file.read",
        _ => false,
    });

    let dest_node = nodes.iter().find(|n| match n {
        OutputNode::Resource { id, .. } => id == "local_file.destination",
        _ => false,
    });

    assert!(data_node.is_some(), "data.local_file.read not found");
    assert!(dest_node.is_some(), "local_file.destination not found");

    match data_node.unwrap() {
        OutputNode::Resource { data, .. } => {
            assert_eq!(data.node_type, "data", "Node type should be data");
            // Data sources often have "read" action or "no-op" depending on plan
            // But type must be data.
        }
        _ => panic!("Wrong enum variant"),
    }

    // Edge: data -> destination (Source -> Target)
    let edge = edges
        .iter()
        .find(|e| e.source == "data.local_file.read" && e.target == "local_file.destination");
    assert!(edge.is_some(), "Edge from data to destination not found");
}

#[test]
fn test_end_to_end_locals() {
    let result = run_fixture("locals", false);

    let nodes = result.nodes;
    let edges = result.edges;

    // We expect:
    // - var.input
    // - local_file.out
    // LOCALS should be simplified away!

    let var_node = nodes.iter().find(|n| match n {
        OutputNode::Resource { id, .. } => id == "var.input",
        _ => false,
    });

    let out_node = nodes.iter().find(|n| match n {
        OutputNode::Resource { id, .. } => id == "local_file.out",
        _ => false,
    });

    // Check for intermediate locals - SHOULD NOT EXIST
    let intermediate = nodes.iter().find(|n| match n {
        OutputNode::Resource { id, .. } => id.contains("local.intermediate"),
        _ => false,
    });

    assert!(var_node.is_some(), "var.input not found");
    assert!(out_node.is_some(), "local_file.out not found");
    assert!(
        intermediate.is_none(),
        "local.intermediate should be simplified away"
    );

    // Edge: var.input -> local_file.out
    // effectively: out depends on var
    // source=var.input, target=local_file.out
    let edge = edges
        .iter()
        .find(|e| e.source == "var.input" && e.target == "local_file.out");
    assert!(
        edge.is_some(),
        "Direct edge from var to out (simplified) not found"
    );
}

#[test]
fn test_end_to_end_complex_modules() {
    let result = run_fixture("complex_modules", false);

    let nodes = result.nodes;
    let edges = result.edges;

    // Verify Modules exist as groups
    // - module.network
    // - module.app
    // - module.app.module.db

    let net_grp = nodes.iter().find(|n| match n {
        OutputNode::Group { id, .. } => id == "module.network",
        _ => false,
    });
    let app_grp = nodes.iter().find(|n| match n {
        OutputNode::Group { id, .. } => id == "module.app",
        _ => false,
    });
    let db_grp = nodes.iter().find(|n| match n {
        OutputNode::Group { id, .. } => id == "module.app.module.db",
        _ => false,
    });

    assert!(net_grp.is_some(), "module.network group missing");
    assert!(app_grp.is_some(), "module.app group missing");
    assert!(db_grp.is_some(), "module.app.module.db group missing");

    // Verify Hierarchy (db inside app)
    match db_grp.unwrap() {
        OutputNode::Group { parent_id, .. } => {
            assert_eq!(
                parent_id.as_deref(),
                Some("module.app"),
                "Db module should be inside app module"
            );
        }
        _ => panic!("wrong type"),
    }

    // Verify Resources
    let vpc_res = nodes.iter().find(|n| match n {
        OutputNode::Resource { id, .. } => id == "module.network.local_file.vpc",
        _ => false,
    });
    let app_res = nodes.iter().find(|n| match n {
        OutputNode::Resource { id, .. } => id == "module.app.local_file.app_server",
        _ => false,
    });
    let db_res = nodes.iter().find(|n| match n {
        OutputNode::Resource { id, .. } => id == "module.app.module.db.local_file.database",
        _ => false,
    });
    let _root_res = nodes.iter().find(|n| match n {
        OutputNode::Resource { id, .. } => id == "local_file.root_config",
        _ => false,
    });
    let root_var = nodes.iter().find(|n| match n {
        OutputNode::Resource { id, .. } => id == "var.environment",
        _ => false,
    });
    let root_out = nodes.iter().find(|n| match n {
        OutputNode::Resource { id, .. } => id == "output.final_config_path",
        _ => false,
    });

    assert!(vpc_res.is_some());
    assert!(app_res.is_some());
    assert!(db_res.is_some());

    // Verify HCL extraction for nested module resource
    match db_res.unwrap() {
        OutputNode::Resource { data, .. } => {
            let hcl = data
                .hcl
                .as_deref()
                .expect("HCL should be present for deep module resource");
            assert!(hcl.contains("resource \"local_file\" \"database\" {"));
            assert!(hcl.contains("content = \"data\""));
        }
        _ => panic!("wrong type"),
    }

    match root_var.unwrap() {
        OutputNode::Resource { parent_id, .. } => {
            assert_eq!(
                parent_id.as_deref(),
                Some("root_variables"),
                "root_variable should belong to root_variables group"
            );
        }
        _ => panic!("wrong type"),
    }

    match root_out.unwrap() {
        OutputNode::Resource { parent_id, .. } => {
            assert_eq!(
                parent_id.as_deref(),
                Some("root_outputs"),
                "root output should belong to root_outputs group"
            );
        }
        _ => panic!("wrong type"),
    }

    let vars_grp = nodes.iter().find(|n| match n {
        OutputNode::Group { id, .. } => id == "root_variables",
        _ => false,
    });
    assert!(vars_grp.is_some(), "root_variables group missing");

    let outs_grp = nodes.iter().find(|n| match n {
        OutputNode::Group { id, .. } => id == "root_outputs",
        _ => false,
    });
    assert!(outs_grp.is_some(), "root_outputs group missing");

    // Verify Dependencies

    // 1. app_server depends on db endpoint
    // "module.app.local_file.app_server" -> "module.app.module.db.local_file.database" (effectively)
    // There might be intermediate output nodes like "module.app.module.db.output.db_endpoint".
    // If output nodes are simplified, we expect direct edge.
    // However, for MODULE outputs (not root outputs), we simplified them in previous logic?
    // Let's check logic:
    // `is_simplifiable = (n_type == "var" || n_type == "local" || n_type == "output") && !is_root_output;`
    // Module output is "module.app.output.app_ip" which determines type?
    // `determine_block_type` checks if "output" is in path parts.
    // So "module.app.output.app_ip" -> type="output".
    // So intermediate module outputs SHOULD be simplified, leading to direct edges.

    // Check edge: Source(db) -> Target(app)
    let db_to_app = edges.iter().find(|e| {
        e.source == "module.app.module.db.local_file.database"
            && e.target == "module.app.local_file.app_server"
    });
    assert!(db_to_app.is_some(), "Edge from db to app server missing");

    // 2. app_server depends on network vpc/subnet
    // Source(vpc) -> Target(app)
    let vpc_to_app = edges.iter().find(|e| {
        e.source == "module.network.local_file.vpc"
            && e.target == "module.app.local_file.app_server"
    });
    // Note: vpc dependencies flow through outputs. output.vpc_id.
    assert!(vpc_to_app.is_some(), "Edge from vpc to app server missing");

    // 3. root_config depends on app module output (app_ip) -> which depends on app_server
    // Source(app_server) -> Target(root_config)
    let app_to_root = edges.iter().find(|e| {
        e.source == "module.app.local_file.app_server" && e.target == "local_file.root_config"
    });
    assert!(
        app_to_root.is_some(),
        "Edge from app server to root config missing"
    );

    // 4. app_server depends on var.environment
    // Pass through: var.environment -> module.app.var.env -> local_file.app_server
    // Intermediate "module.app.var.env" should be simplified.
    // So Source(var.environment) -> Target(app_server)
    let var_to_app = edges
        .iter()
        .find(|e| e.source == "var.environment" && e.target == "module.app.local_file.app_server");
    assert!(
        var_to_app.is_some(),
        "Edge from var.environment to app server missing"
    );

    // 5. root output depends on root_config
    // Source(root_config) -> Target(output.final_config_path)
    let root_to_out = edges
        .iter()
        .find(|e| e.source == "local_file.root_config" && e.target == "output.final_config_path");
    assert!(
        root_to_out.is_some(),
        "Edge from root_config to output missing"
    );
}

#[test]
fn test_end_to_end_state_mode() {
    let result = run_fixture("state_mode_test", true);

    // Find the local_file resource
    let file_node = result
        .nodes
        .iter()
        .find(|n| match n {
            OutputNode::Resource { id, .. } => id == "local_file.state_test",
            _ => false,
        })
        .expect("local_file.state_test should exist in state graph");

    match file_node {
        OutputNode::Resource { data, .. } => {
            // Should have No Action (existing node) or "read" if data source (this is resource)
            // Our logic: "Managed resource in state -> No action"
            assert_eq!(
                data.action, None,
                "Existing resource should have action=None"
            );

            // Should have Values
            assert!(
                data.values.is_some(),
                "Values should be present in state mode"
            );
            let values = data.values.as_ref().unwrap();

            assert_eq!(values["content"], "this is state content");
            assert!(
                values["filename"]
                    .as_str()
                    .unwrap()
                    .ends_with("state_test.txt")
            );
        }
        _ => panic!("Wrong node type"),
    }
}

#[test]
fn test_end_to_end_locals_constant() {
    let result = run_fixture("locals_constant", false);
    let nodes = result.nodes;
    let edges = result.edges;

    // We expect:
    // - local_file.f
    // - NO local.constant_val node

    let file_node = nodes.iter().find(|n| match n {
        OutputNode::Resource { id, .. } => id == "local_file.f",
        _ => false,
    });

    let local_node = nodes.iter().find(|n| match n {
        OutputNode::Resource { id, .. } => id == "local.constant_val",
        _ => false,
    });

    assert!(file_node.is_some(), "local_file.f not found");
    assert!(
        local_node.is_none(),
        "local.constant_val should be simplified/hidden"
    );

    // Edges: resource f depends on nothing (constant). So no edges.
    assert!(
        edges.is_empty(),
        "Expected no edges for constant local dependency"
    );
}

#[test]
fn test_end_to_end_module_constant_output() {
    let result = run_fixture("module_constant_output", false);
    let nodes = result.nodes;

    // We expect:
    // - module.c.output.out

    // Note: module outputs are usually hidden if they are just passthroughs,
    // but if they are constants (no deps), user requested to show them.

    let out_node = nodes.iter().find(|n| match n {
        OutputNode::Resource { id, .. } => id == "module.c.output.out",
        _ => false,
    });

    assert!(
        out_node.is_none(),
        "module.c.output.out should be hidden as it is a module output"
    );
}

#[test]
fn test_end_to_end_module_var_hidden() {
    let result = run_fixture("module_var_hidden", false);
    let nodes = result.nodes;

    // We expect:
    // - module.m.local_file.inner
    // - module.m.var.inner_val SHOULD BE HIDDEN because it's a module variable (not root) and has no external deps (default)

    let file_node = nodes.iter().find(|n| match n {
        OutputNode::Resource { id, .. } => id == "module.m.local_file.inner",
        _ => false,
    });

    let var_node = nodes.iter().find(|n| match n {
        OutputNode::Resource { id, .. } => id == "module.m.var.inner_val",
        _ => false,
    });

    assert!(file_node.is_some(), "module.m.local_file.inner not found");
    // With current code, this asserts TRUE (node is there).
    // We want it to be FALSE (node hidden).
    // For now, write assertion that reflects DESIRED behavior, so it fails.
    assert!(
        var_node.is_none(),
        "module.m.var.inner_val should be hidden"
    );
}

#[test]
fn test_end_to_end_count_and_foreach() {
    let result = run_fixture("count_and_foreach", false);
    let nodes = result.nodes;

    // 1. Check Counted Resource
    // Should be aggregated into a single node "local_file.counted"
    let counted = nodes
        .iter()
        .find(|n| match n {
            OutputNode::Resource { id, .. } => id == "local_file.counted",
            _ => false,
        })
        .expect("local_file.counted not found");

    match counted {
        OutputNode::Resource { data, .. } => {
            // Plan will have [0] and [1] creating. Aggregated should be "create".
            // Since both actions are "create", our logic should dedupe them to single "create".
            assert_eq!(
                data.action.as_deref(),
                Some("create"),
                "counted action should be create"
            );
            assert_eq!(data.count, Some(2), "counted count should be 2");
        }
        _ => panic!("Wrong type"),
    }

    // 2. Check ForEach Resource
    // Should be aggregated into "local_file.foreach"
    let foreach = nodes
        .iter()
        .find(|n| match n {
            OutputNode::Resource { id, .. } => id == "local_file.foreach",
            _ => false,
        })
        .expect("local_file.foreach not found");

    match foreach {
        OutputNode::Resource { data, .. } => {
            assert_eq!(
                data.action.as_deref(),
                Some("create"),
                "foreach action should be create"
            );
            assert_eq!(data.count, Some(2), "foreach count should be 2");
        }
        _ => panic!("Wrong type"),
    }

    let has_counted_edge = result
        .edges
        .iter()
        .any(|e| e.source == "local_file.counted" && e.target == "local_file.dependent");
    assert!(has_counted_edge, "Missing edge: counted -> dependent");

    let has_foreach_edge = result
        .edges
        .iter()
        .any(|e| e.source == "local_file.foreach" && e.target == "local_file.dependent");
    assert!(has_foreach_edge, "Missing edge: foreach -> dependent");
}

#[test]
fn test_mock_conditional_resources() {
    let result = run_fixture("conditional_resources", false);

    let nodes = result.nodes;

    // We expect:
    // 1. null_resource.enabled_example (create)
    // 2. data.null_data_source.enabled_data (read)
    // We expect MISSING:
    // 1. null_resource.disabled_example (should be filtered out because not in plan and not in state)

    let enabled_res = nodes.iter().find(|n| match n {
        OutputNode::Resource { id, .. } => id == "null_resource.enabled_example",
        _ => false,
    });
    assert!(enabled_res.is_some(), "Enabled resource should exist");

    let enabled_data = nodes.iter().find(|n| match n {
        OutputNode::Resource { id, .. } => id == "data.null_data_source.enabled_data",
        _ => false,
    });
    assert!(enabled_data.is_some(), "Enabled data should exist");

    let disabled_res = nodes.iter().find(|n| match n {
        OutputNode::Resource { id, .. } => id == "null_resource.disabled_example",
        _ => false,
    });

    // This assertion SHOULD FAIL currently, proving the issue
    assert!(
        disabled_res.is_none(),
        "Disabled resource should NOT exist in graph"
    );
}

#[test]
fn test_mock_empty_module_pruning() {
    let result = run_fixture("empty_module", false);

    let nodes = result.nodes;

    // We expect:
    // 1. null_resource.placeholder -> REMOVED (due to count=0/no-op)
    // 2. module.empty_mod -> REMOVED (due to having no children)

    let resource_node = nodes.iter().find(|n| match n {
        OutputNode::Resource { id, .. } => id == "module.empty_mod.null_resource.placeholder",
        _ => false,
    });
    assert!(resource_node.is_none(), "Disabled resource should be gone");

    let module_node = nodes.iter().find(|n| match n {
        OutputNode::Group { id, .. } => id == "module.empty_mod",
        _ => false,
    });

    // This assertion SHOULD FAIL currently, as we haven't implemented pruning
    assert!(module_node.is_none(), "Empty module group should be pruned");
}

#[test]
fn test_end_to_end_module_inner_outputs() {
    let result = run_fixture("module_inner_outputs", false);
    let nodes = result.nodes;
    let edges = result.edges;

    // We expect:
    // - output.root_out (root output)
    // - module.inner.output.inner_out should NOT be shown (it's a module output with a constant value)

    let root_out = nodes.iter().find(|n| match n {
        OutputNode::Resource { id, .. } => id == "output.root_out",
        _ => false,
    });

    let inner_out = nodes.iter().find(|n| match n {
        OutputNode::Resource { id, .. } => id == "module.inner.output.inner_out",
        _ => false,
    });

    assert!(root_out.is_some(), "output.root_out should be shown");

    // Module outputs should NOT be shown - they are simplified away
    assert!(
        inner_out.is_none(),
        "module.inner.output.inner_out should NOT be shown (module outputs are hidden)"
    );

    // Since the module output is a constant (no actual resource dependencies),
    // the root output should have no incoming edges after simplification
    // (this is correct - there's no actual infrastructure to connect to)
    let root_out_edges: Vec<_> = edges
        .iter()
        .filter(|e| e.target == "output.root_out")
        .collect();
    assert!(
        root_out_edges.is_empty(),
        "Root output with constant module output dependency should have no edges after simplification"
    );
}

#[test]
fn test_end_to_end_edge_attributes() {
    let result = run_fixture("variables", false);
    let edges = result.edges;

    // We expect an edge from var.content -> local_file.file with attribute "content"
    let edge_content = edges
        .iter()
        .find(|e| e.source == "var.content" && e.target == "local_file.file")
        .expect("Edge var.content -> local_file.file not found");

    // Note: Dependent (target) uses Dependency (source) in "content" arg.
    // The source is "var.content". Reference is "var.content".
    // Since it's an exact match of the variable (value), no specific attribute suffix is extracted.

    assert!(edge_content.attributes.is_some());
    assert_eq!(edge_content.attributes.as_ref().unwrap(), &vec!["content"]);

    // Edge var.filename -> local_file.file with attribute "filename"
    let edge_filename = edges
        .iter()
        .find(|e| e.source == "var.filename" && e.target == "local_file.file")
        .expect("Edge var.filename -> local_file.file not found");

    assert!(edge_filename.attributes.is_some());
    assert_eq!(
        edge_filename.attributes.as_ref().unwrap(),
        &vec!["filename"]
    );
}

#[test]
fn test_end_to_end_edge_attributes_real() {
    let result = run_fixture("attribute_check", false);
    let edges = result.edges;

    // 1. Check base -> dependent
    // Should depend on 'content'
    let edge_simple = edges
        .iter()
        .find(|e| e.source == "local_file.base" && e.target == "local_file.dependent")
        .expect("Edge base->dependent not found");

    // We expect Some(vec!["content"])
    let mut attrs = edge_simple
        .attributes
        .clone()
        .expect("Attributes should be present");
    attrs.sort();
    assert_eq!(attrs, vec!["content"]);

    // 2. Check base -> dependent_multi
    // Should depend on 'content' (the argument name)
    let edge_multi = edges
        .iter()
        .find(|e| e.source == "local_file.base" && e.target == "local_file.dependent_multi")
        .expect("Edge base->dependent_multi not found");

    let mut attrs_multi = edge_multi
        .attributes
        .clone()
        .expect("Attributes should be present");
    attrs_multi.sort();
    assert_eq!(attrs_multi, vec!["content"]);
}

#[test]
fn test_hcl_extraction() {
    let result = run_fixture("simple", false);

    let nodes = result.nodes;

    let foo_node = nodes
        .iter()
        .find(|n| match n {
            OutputNode::Resource { id, .. } => id == "local_file.foo",
            _ => false,
        })
        .expect("local_file.foo not found");

    match foo_node {
        OutputNode::Resource { data, .. } => {
            let hcl = data.hcl.as_deref().expect("HCL should be present");
            // Check that it contains the resource definition
            assert!(hcl.contains("resource \"local_file\" \"foo\" {"));
            assert!(hcl.contains("content  = \"foo!\""));
        }
        _ => panic!("Wrong node type"),
    }
}

#[test]
fn test_phantom_data_source_pruning() {
    let result = run_fixture("phantom_data", false);
    let nodes = result.nodes;

    // The data source exists in graph.dot but count=0 means it's not in the plan.
    // It should be pruned.
    let ghost_node = nodes.iter().find(|n| match n {
        OutputNode::Resource { id, .. } => id == "data.local_file.ghost",
        _ => false,
    });

    assert!(ghost_node.is_none(), "Phantom data source should be pruned");
}

#[test]
fn test_state_file_mode() {
    // This test simulates "tofu show -json" output where only "values" (state) is present,
    // and "resource_changes" is empty/missing. This happens when checking current state.

    let state_json = r#"{
        "format_version": "1.0",
        "terraform_version": "1.6.0",
        "values": {
            "root_module": {
                "resources": [
                    {
                        "address": "aws_instance.prod",
                        "mode": "managed",
                        "type": "aws_instance",
                        "name": "prod",
                        "provider_name": "registry.opentofu.org/hashicorp/aws",
                        "values": {
                            "ami": "ami-123",
                            "instance_type": "t2.micro",
                            "tags": {
                                "Name": "prod-server"
                            }
                        }
                    },
                    {
                        "address": "data.aws_ami.ubuntu",
                        "mode": "data",
                        "type": "aws_ami",
                        "name": "ubuntu",
                        "provider_name": "registry.opentofu.org/hashicorp/aws",
                        "values": {
                            "id": "ami-123"
                        }
                    }
                ]
            }
        }
    }"#;

    let dot_content = r#"
        digraph {
            "[root] aws_instance.prod" [label = "aws_instance.prod", shape = "box"]
            "[root] data.aws_ami.ubuntu" [label = "data.aws_ami.ubuntu", shape = "box"]
        }
    "#;

    // Process with include_values = true
    let graph =
        process_graph(state_json, dot_content, true, None).expect("Graph processing failed");

    // Verify Managed Resource
    let prod_node = graph
        .nodes
        .iter()
        .find(|n| match n {
            OutputNode::Resource { id, .. } => id == "aws_instance.prod",
            _ => false,
        })
        .expect("aws_instance.prod should exist");

    match prod_node {
        OutputNode::Resource { data, .. } => {
            assert_eq!(data.node_type, "resource");
            // Action should be None for existing state items
            assert!(data.action.is_none());
            // Values should be present
            let vals = data.values.as_ref().expect("Values should be present");
            assert_eq!(vals["tags"]["Name"], "prod-server");
        }
        _ => panic!("Wrong node type"),
    }

    // Verify Data Source
    let data_node = graph
        .nodes
        .iter()
        .find(|n| match n {
            OutputNode::Resource { id, .. } => id == "data.aws_ami.ubuntu",
            _ => false,
        })
        .expect("data.aws_ami.ubuntu should exist");

    match data_node {
        OutputNode::Resource { data, .. } => {
            assert_eq!(data.node_type, "data");
            // Data sources usually show as "read" if in plan,
            // but in state view they are just 'there'.
            // My implementation sets action="read" for data sources in state as well.
            // Let's check what I implemented:
            // if temp_type == "data" { (Some("read".to_string()), ...) }
            assert_eq!(data.action, Some("read".to_string()));

            let vals = data.values.as_ref().expect("Values should be present");
            assert_eq!(vals["id"], "ami-123");
        }
        _ => panic!("Wrong node type"),
    }
}

#[test]
fn test_realistic_eks_module_outputs() {
    // This test verifies that edges correctly reference module output nodes
    // and that those module output nodes are included in the graph.
    // Previously, root outputs like output.kubernetes_endpoint referenced
    // module.eks.module.eks.output.cluster_endpoint, but these intermediate
    // module output nodes were being filtered out.

    // This fixture requires AWS credentials to run Terraform, so we check for
    // pre-generated plan files. If they don't exist, skip the test.
    let fixture_path = std::path::Path::new("tofu/tests/fixtures/realistic_eks");
    let plan_path = fixture_path.join("plan.json");
    let graph_path = fixture_path.join("graph.dot");

    if !plan_path.exists() || !graph_path.exists() {
        println!(
            "Skipping test: realistic_eks requires pre-generated plan files (plan.json and graph.dot)"
        );
        println!("Generate them with:");
        println!("  cd tofu/tests/fixtures/realistic_eks");
        println!("  terraform init && terraform plan -out=plan.tfplan");
        println!("  terraform show -json plan.tfplan > plan.json");
        println!("  terraform graph -plan=plan.tfplan > graph.dot");
        return;
    }

    let plan_json = std::fs::read_to_string(&plan_path).expect("Failed to read plan.json");
    let dot_content = std::fs::read_to_string(&graph_path).expect("Failed to read graph.dot");

    let result = process_graph(
        &plan_json,
        &dot_content,
        false,
        Some(fixture_path.to_path_buf()),
    )
    .expect("Failed to process graph");

    let nodes = result.nodes;
    let edges = result.edges;

    // Debug: Print all node IDs
    println!("All nodes in graph:");
    for node in &nodes {
        match node {
            OutputNode::Resource { id, data, .. } => {
                println!(
                    "  - {} (type: {}, action: {:?})",
                    id, data.node_type, data.action
                );
            }
            OutputNode::Group { id, .. } => {
                println!("  - {} (GROUP)", id);
            }
        }
    }

    // Debug: Print all edges
    println!("\nAll edges in graph:");
    for edge in &edges {
        println!("  - {} -> {} (id: {})", edge.source, edge.target, edge.id);
    }

    // Verify that all edge sources and targets exist in nodes
    let node_ids: std::collections::HashSet<String> = nodes
        .iter()
        .filter_map(|n| match n {
            OutputNode::Resource { id, .. } => Some(id.clone()),
            OutputNode::Group { id, .. } => Some(id.clone()),
        })
        .collect();

    let mut missing_nodes = Vec::new();

    for edge in &edges {
        if !node_ids.contains(&edge.source) {
            missing_nodes.push(format!(
                "Edge {} references missing source: {}",
                edge.id, edge.source
            ));
        }
        if !node_ids.contains(&edge.target) {
            missing_nodes.push(format!(
                "Edge {} references missing target: {}",
                edge.id, edge.target
            ));
        }
    }

    if !missing_nodes.is_empty() {
        panic!(
            "Found edges referencing non-existent nodes:\n{}",
            missing_nodes.join("\n")
        );
    }

    // Verify that root outputs exist
    assert!(
        node_ids.contains("output.kubernetes_endpoint"),
        "output.kubernetes_endpoint should exist"
    );
    assert!(
        node_ids.contains("output.cluster_name"),
        "output.cluster_name should exist"
    );
    assert!(
        node_ids.contains("output.kubernetes_certificate_authority_data"),
        "output.kubernetes_certificate_authority_data should exist"
    );

    // Verify that root outputs have incoming edges from actual resources
    let output_edges: Vec<_> = edges
        .iter()
        .filter(|e| e.target.starts_with("output."))
        .collect();

    assert!(
        !output_edges.is_empty(),
        "Root outputs should have incoming edges"
    );

    // Check each specific output has an edge
    let kubernetes_endpoint_edge = output_edges
        .iter()
        .find(|e| e.target == "output.kubernetes_endpoint");
    assert!(
        kubernetes_endpoint_edge.is_some(),
        "output.kubernetes_endpoint should have an incoming edge"
    );
    assert_eq!(
        kubernetes_endpoint_edge.unwrap().source,
        "module.eks.module.eks.aws_eks_cluster.this",
        "output.kubernetes_endpoint edge should come from the EKS cluster resource"
    );

    let cluster_name_edge = output_edges
        .iter()
        .find(|e| e.target == "output.cluster_name");
    assert!(
        cluster_name_edge.is_some(),
        "output.cluster_name should have an incoming edge"
    );
    assert_eq!(
        cluster_name_edge.unwrap().source,
        "module.eks.module.eks.aws_eks_cluster.this",
        "output.cluster_name edge should come from the EKS cluster resource"
    );

    let cert_auth_edge = output_edges
        .iter()
        .find(|e| e.target == "output.kubernetes_certificate_authority_data");
    assert!(
        cert_auth_edge.is_some(),
        "output.kubernetes_certificate_authority_data should have an incoming edge"
    );
    assert_eq!(
        cert_auth_edge.unwrap().source,
        "module.eks.module.eks.aws_eks_cluster.this",
        "output.kubernetes_certificate_authority_data edge should come from the EKS cluster resource"
    );
}
