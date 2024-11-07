use env_defs::{ModuleExample, ModuleSpec};

pub fn generate_module_example_deployment(module: &ModuleSpec, module_example: &ModuleExample) -> serde_yaml::Value {
    let mut manifest: serde_yaml::Value = serde_yaml::from_str(&format!(r#"
apiVersion: infrabridge.io/v1
kind: {}
metadata:
  name: {}
  # namespace: infrabridge_cli
spec:
  moduleVersion: {}
"#, module.module_name, module_example.name, module.version.as_ref().unwrap(),)).unwrap();

    manifest["spec"]["variables"] = serde_yaml::to_value(module_example.variables.clone()).unwrap();

    manifest
}