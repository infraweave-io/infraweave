use std::path::Path;

use chrono::Local;
use env_defs::{
    DeploymentManifest, ModuleManifest, ModuleResp, StackManifest, TfOutput, TfVariable,
};
use env_utils::{get_zip_file_from_str, read_stack_directory, zero_pad_semver};

use crate::{api_module::{_get_latest_module_version, _list_module}, get_module_version};

pub fn generate_terraform_module(claim_modules: &Vec<(DeploymentManifest, ModuleResp)>) -> String {
    let mut terraform_module = String::new();

    terraform_module.push_str("# Generated Terraform module\n");

    terraform_module.push_str(
        r##"
terraform {
    backend "s3" {}
}

"##,
    );

    // -------------------------
    // Generate the variable block
    // -------------------------
    for (claim, module) in claim_modules.iter() {
        for variable in &module.tf_variables {
            terraform_module.push_str(&format!(
                "variable \"{}\" {{\n",
                get_variable_name(claim, variable),
            ));

            if let Some(description) = &variable.description {
                terraform_module.push_str(&format!("  description = \"{}\"\n", description,));
            }

            if !&variable._type.to_string().is_empty() {
                terraform_module
                    .push_str(&format!("  type = {}\n", variable._type.as_str().unwrap()));
                // TODO cautious, need to verify this with https://developer.hashicorp.com/terraform/language/expressions/types
            }

            if let Some(nullable) = &variable.nullable {
                terraform_module.push_str(&format!("  nullable = {}\n", &nullable.to_string()));
            }

            if let Some(sensitive) = &variable.sensitive {
                terraform_module.push_str(&format!("  sensitive = {}\n", &sensitive.to_string()));
            }

            if let Some(default_value) = &variable.default {
                terraform_module.push_str(&format!(
                    "  default = {}\n",
                    format_value(default_value, &variable._type.to_string())
                ));
            }

            terraform_module.push_str("}\n\n");
        }
    }

    // -------------------------
    // Generate the module block
    // -------------------------
    for (claim, module) in claim_modules.iter() {
        terraform_module.push_str(&format!(
            "module \"{}\" {{\n",
            module.manifest.metadata.name
        ));
        terraform_module.push_str(&format!(
            "  source = \"./{}\"\n\n",
            module.s3_key.trim_end_matches(".zip")
        ));

        for variable in &module.tf_variables {
            terraform_module.push_str(&format!(
                "  {} = var.{}__{}\n",
                variable.name, module.module, variable.name
            ));
        }

        terraform_module.push_str("}\n\n");
    }

    // -------------------------
    // Generate the output block
    // -------------------------
    for (claim, module) in claim_modules.iter() {
        for output in &module.tf_outputs {
            terraform_module.push_str(&format!(
                "output \"{}\" {{\n",
                get_output_name(claim, output),
            ));
            terraform_module.push_str(&format!(
                "  description = \"{}\"\n",
                output.description.clone(),
            ));
            terraform_module.push_str(&format!(
                "  value = module.{}.{}\n",
                module.manifest.metadata.name, output.name
            ));
            terraform_module.push_str("}\n\n");
        }
    }

    terraform_module
}

fn get_variable_name(claim: &DeploymentManifest, variable: &TfVariable) -> String {
    format!("{}__{}", claim.metadata.name, variable.name)
}

fn get_output_name(claim: &DeploymentManifest, output: &TfOutput) -> String {
    format!("{}__{}", claim.metadata.name, output.name)
}

fn format_value(value: &serde_json::Value, value_type: &str) -> String {
    match value {
        serde_json::Value::String(s) => format!("\"{}\"", s),
        serde_json::Value::Number(n) => n.to_string(),
        serde_json::Value::Bool(b) => b.to_string(),
        serde_json::Value::Array(arr) => {
            let formatted_values: Vec<String> =
                arr.iter().map(|v| format_value(v, value_type)).collect();
            format!("[{}]", formatted_values.join(", "))
        }
        serde_json::Value::Object(_) => format!("{{}}"), // Handle maps if needed
        _ => "null".to_string(),
    }
}

pub async fn publish_stack(manifest_path: &String, environment: &String) -> anyhow::Result<(), anyhow::Error> {
    println!("Publishing stack from {}", manifest_path);
    let stack_manifest = get_stack_manifest(manifest_path).await;
    let claims = get_claims_in_stack(manifest_path).await;
    let claim_modules = get_modules_in_stack(&claims).await;

    let terraform_module = generate_terraform_module(&claim_modules);

    let tf_variables = claim_modules
        .iter()
        .flat_map(|(claim, module)| {
            module.tf_variables.clone().into_iter().map(|mut variable| {
                variable.name = get_variable_name(claim, &variable);
                variable
            })
        })
        .collect::<Vec<TfVariable>>();

    let tf_outputs = claim_modules
        .iter()
        .flat_map(|(claim, module)| {
            module.tf_outputs.clone().into_iter().map(|mut output| {
                output.name = get_output_name(claim, &output);
                output
            })
        })
        .collect::<Vec<TfOutput>>();

    let module_manifest = ModuleManifest {
        metadata: env_defs::Metadata {
            name: stack_manifest.metadata.name.clone(),
        },
        kind: stack_manifest.kind.clone(),
        spec: env_defs::ModuleSpec {
            module_name: stack_manifest.spec.stack_name.clone(),
            version: stack_manifest.spec.version.clone(),
            description: stack_manifest.spec.description.clone(),
            reference: stack_manifest.spec.reference.clone(),
        },
        api_version: stack_manifest.api_version.clone(),
    };

    let stack_data = Some(env_defs::ModuleStackData {
        modules: claim_modules
            .iter()
            .map(|(c, m)| env_defs::StackModule {
                module: m.module.clone(),
                version: m.version.clone(),
                s3_key: m.s3_key.clone(),
            })
            .collect(),
    });

    let module = ModuleResp {
        environment: environment.clone(),
        environment_version: format!(
            "{}#{}",
            environment.clone(),
            zero_pad_semver(stack_manifest.spec.version.as_str(), 3).unwrap()
        ),
        version: stack_manifest.spec.version.clone(),
        timestamp: Local::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
        module: stack_manifest.metadata.name.clone(),
        module_name: stack_manifest.spec.stack_name.clone(),
        description: stack_manifest.spec.description.clone(),
        reference: stack_manifest.spec.reference.clone(),
        manifest: module_manifest,
        tf_variables: tf_variables,
        tf_outputs: tf_outputs,
        s3_key: format!(
            "{}/{}-{}.zip",
            &stack_manifest.metadata.name,
            &stack_manifest.metadata.name,
            &stack_manifest.spec.version
        ), // s3_key -> "{module}/{module}-{version}.zip"
        stack_data: stack_data,
    };

    let zip_file = get_zip_file_from_str(&terraform_module, "main.tf").unwrap();
    let zip_base64 = base64::encode(&zip_file);

    println!("Uploading stack as module {}", terraform_module);
    crate::api_module::upload_module(&module, &zip_base64, &environment).await
}

async fn get_claims_in_stack(manifest_path: &String) -> Vec<DeploymentManifest> {
    println!("Reading stack claim manifests in {}", manifest_path);
    let claims =
        read_stack_directory(Path::new(manifest_path)).expect("Failed to read stack directory");
    claims
}

async fn get_stack_manifest(manifest_path: &String) -> StackManifest {
    println!("Reading stack manifest in {}", manifest_path);
    let stack_yaml_path = Path::new(manifest_path).join("stack.yaml");
    let manifest =
        std::fs::read_to_string(&stack_yaml_path).expect("Failed to read stack manifest file");
    let stack_yaml =
        serde_yaml::from_str::<StackManifest>(&manifest).expect("Failed to parse stack manifest");
    stack_yaml
}

async fn get_modules_in_stack(
    deployment_manifests: &Vec<DeploymentManifest>,
) -> Vec<(DeploymentManifest, ModuleResp)> {
    println!("Getting modules for deployment manifests");
    let mut claim_modules: Vec<(DeploymentManifest, ModuleResp)> = vec![];

    for claim in deployment_manifests {
        let environment = "dev".to_string();
        let module = claim.kind.to_lowercase();
        let version = claim.spec.module_version.to_string();
        let module_resp = get_module_version(&module, &environment, &version)
            .await
            .unwrap();
        claim_modules.push((claim.clone(), module_resp));
    }

    claim_modules
}

pub async fn get_latest_stack_version(
    module: &String,
    environment: &String,
) -> anyhow::Result<ModuleResp> {
    _get_latest_module_version("LATEST_STACK", module, environment).await
}

pub async fn list_stack(environment: &str) -> Result<Vec<ModuleResp>, anyhow::Error> {
    _list_module("LATEST_STACK", environment).await
}

pub async fn get_stack_version(module: &String, environment: &String, version: &String) -> anyhow::Result<ModuleResp> {
    crate::api_module::get_module_version(module, environment, version).await
}

pub async fn get_all_stack_versions(module: &str, environment: &str) -> Result<Vec<ModuleResp>, anyhow::Error> {
    crate::api_module::get_all_module_versions(module, environment).await
}
