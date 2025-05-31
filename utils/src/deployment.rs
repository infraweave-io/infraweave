use crate::to_camel_case;
use env_defs::{DeploymentResp, ModuleExample, ModuleResp, ModuleSpec};

pub fn generate_module_example_deployment(
    module: &ModuleSpec,
    module_example: &ModuleExample,
) -> serde_yaml::Value {
    let mut manifest: serde_yaml::Value = serde_yaml::from_str(&format!(
        r#"
apiVersion: infraweave.io/v1
kind: {}
metadata:
  name: {}
  # namespace: infraweave_cli
spec:
  moduleVersion: {}
"#,
        module.module_name,
        module_example.name,
        module.version.as_ref().unwrap(),
    ))
    .unwrap();

    manifest["spec"]["variables"] = serde_yaml::to_value(module_example.variables.clone()).unwrap();

    manifest
}

pub fn generate_deployment_claim(deployment: &DeploymentResp, module: &ModuleResp) -> String {
    let variables = match &deployment.module_type.as_str() {
        &"stack" => deployment.variables.clone(),
        &"module" => {
            let mut vars = serde_json::Map::new();
            for (key, value) in deployment.variables.as_object().unwrap().iter() {
                vars.insert(to_camel_case(key), value.clone());
            }
            let vars = serde_json::Value::Object(vars);
            vars
        }
        _ => panic!("Unsupported module type: {}", deployment.module_type),
    };

    format!(
        r#"
apiVersion: infraweave.io/v1
kind: {}
metadata:
  name: {}
  namespace: {}
spec:
  {}
  region: {}
  variables:
{}
"#,
        module.module_name,
        deployment.deployment_id.split("/").last().unwrap(),
        deployment.environment,
        if module.module_type == "stack" {
            format!("stackVersion: {}", &module.version)
        } else {
            format!("moduleVersion: {}", &module.version)
        },
        deployment.region,
        serde_yaml::to_string(&variables)
            .unwrap()
            .trim_start_matches("---\n")
            .lines()
            .map(|line| format!("    {}", line))
            .collect::<Vec<String>>()
            .join("\n"),
    )
}
