use chrono::Local;
use env_defs::{
    DeploymentManifest, ModuleManifest, ModuleResp, StackManifest, TfOutput, TfVariable,
};
use env_utils::{get_outputs_from_tf_files, get_variables_from_tf_files, get_zip_file_from_str, merge_zips, read_stack_directory, to_camel_case, to_snake_case, zero_pad_semver};
use regex::Regex;
use std::{
    collections::HashMap,
    path::Path,
};

use crate::{
    api_module::{_get_latest_module_version, _list_module}, get_module_download_url, get_module_version
};

pub fn generate_full_terraform_module(claim_modules: &Vec<(DeploymentManifest, ModuleResp)>) -> (String, String, String) {

    let variable_collection = collect_module_variables(&claim_modules);
    let output_collection = collect_module_outputs(&claim_modules);
    let module_collection = collect_modules(&claim_modules);

    // Create list of all dependencies between modules
    // Maps every "{{ ModuleName::DeploymentName::OutputName }}" to the output key such as "module.DeploymentName.OutputName"
    let dependency_map = generate_dependency_map(&variable_collection, &output_collection);

    let terraform_module_code = generate_terraform_modules(
        &module_collection,
        &variable_collection,
        &dependency_map,
    );

    let terraform_variable_code =
        generate_terraform_variables(&variable_collection, &dependency_map);

    let terraform_output_code = generate_terraform_outputs(&output_collection, &dependency_map);

    (terraform_module_code, terraform_variable_code, terraform_output_code)
}

fn generate_terraform_modules(
    module_collection: &HashMap<String, ModuleResp>,
    variable_collection: &HashMap<String, TfVariable>,
    dependency_map: &HashMap<String, String>,
) -> String {
    let mut terraform_modules = vec![];

    for (claim_name, module) in module_collection {
        let module_str =
            generate_terraform_module_single(&claim_name, &module,  &variable_collection, &dependency_map);
        terraform_modules.push(module_str);
    }

    terraform_modules.sort(); // Sort for consistent ordering
    terraform_modules.join("\n")
}

fn generate_terraform_module_single(
    claim_name: &str,
    module: &ModuleResp,
    variable_collection: &HashMap<String, TfVariable>,
    dependency_map: &HashMap<String, String>,
) -> String {
    let mut module_str = String::new();
    let source = module.s3_key.split('/').last().unwrap().trim_end_matches(".zip");
    module_str.push_str(
        format!(
            "\nmodule \"{}\" {{\n  source = \"./{}\"\n",
            to_snake_case(claim_name),
            source,
        ).as_str()
    );
    
    let variable_collection: std::collections::BTreeMap<_, _> = variable_collection.into_iter().collect(); // Not necessary, but for consistent ordering of variables

    for (variable_name, _variable_value) in variable_collection {
        let parts = variable_name.split("__").collect::<Vec<&str>>();
        let part_claim_name = parts[0];
        let part_var_name = parts[1];

        if part_claim_name != claim_name { // Skip if variable is not for this module
            continue;
        }

        if dependency_map.contains_key(variable_name) {
            let dependency_str = dependency_map.get(variable_name).unwrap();
            let variable_str = format!("\n  {} = {}", part_var_name, dependency_str);
            // if can be parses as json, then parse it and print as hcl
            if let Ok(value) = serde_json::from_str(dependency_str) {
                let hcl_value = map_value_to_hcl(value);
                let variable_str = format!("\n  {} = {}", part_var_name, hcl_value);
                module_str.push_str(&variable_str);
            } else {
                module_str.push_str(&variable_str);
            }
        } else {
            let variable_str = format!("\n  {} = var.{}", part_var_name, variable_name);
            module_str.push_str(&variable_str);
        }
    }
    module_str.push_str("\n}");
    module_str
}


fn generate_terraform_outputs(
    output_collection: &HashMap<String, TfOutput>,
    dependency_map: &HashMap<String, String>,
) -> String {
    let mut terraform_outputs = vec![];

    for (output_name, output_value) in output_collection {
        let output_str =
            generate_terraform_output_single(output_name, &output_value, &dependency_map);
        terraform_outputs.push(output_str);
    }

    terraform_outputs.sort(); // Sort for consistent ordering
    terraform_outputs.join("\n")
}

fn generate_terraform_output_single(
    output_name: &str,
    _output: &TfOutput,
    _dependency_map: &HashMap<String, String>,
) -> String {
    let var_name = output_name;
    let parts = var_name.split("__").collect::<Vec<&str>>();
    let claim_name = parts[0];
    let output_name = parts[1];
    format!(
        "\noutput \"{}\" {{\n  value = module.{}.{}\n}}",
        var_name, &claim_name, &output_name
    )
}

fn generate_terraform_variables(
    variable_collection: &HashMap<String, TfVariable>,
    dependency_map: &HashMap<String, String>,
) -> String {
    let mut terraform_variables = vec![];

    for (variable_name, variable_value) in variable_collection {
        if dependency_map.contains_key(variable_name) {
            continue;
        }
        let variable_str =
            generate_terraform_variable_single(variable_name, &variable_value, &dependency_map);
        terraform_variables.push(variable_str);
    }

    terraform_variables.sort(); // Sort for consistent ordering
    terraform_variables.join("\n")
}

fn map_value_to_hcl(value: serde_json::Value) -> String {
    match value {
        serde_json::Value::String(_) => {
            format!("\"{}\"", value.as_str().unwrap().to_string())
        }
        serde_json::Value::Number(_) => {
            value.as_str().unwrap().to_string()
        }
        serde_json::Value::Bool(_) => {
            format!("{}", value)
        }
        serde_json::Value::Array(_) => {
            let val = hcl::to_string(&value).unwrap();
            let val = val.replace("$$", "$"); // The hcl library escapes $ as $$
            format!("[\n{}\n  ]", indent(&val, 2))
        }
        serde_json::Value::Object(_) => {
            let val = hcl::to_string(&value).unwrap();
            let val = val.replace("$$", "$"); // The hcl library escapes $ as $$
            format!("{{\n{}\n  }}", indent(&val, 2))
        }
        _ => {
            panic!("Unhandled value type");
        }
    }
}

fn generate_terraform_variable_single(
    variable_name: &str,
    variable: &TfVariable,
    dependency_map: &HashMap<String, String>,
) -> String {
    let var_name = variable_name;
    let in_dependency_map = dependency_map.contains_key(var_name);    

    let default_value: String = if in_dependency_map {
        dependency_map.get(var_name).unwrap().to_string()
    } else {
        match &variable.default {
            Some(value) => {
                map_value_to_hcl(value.clone())
            }
            None => "null".to_string(),
        }
    };
    let _type = variable._type.to_string();
    let _type = _type.trim_matches('"'); // remove quotes from type
    format!(
        "\nvariable \"{}\" {{\n  type = {}\n  default = {}\n}}",
        var_name, _type, &default_value,
    )
}

fn indent(s: &str, level: usize) -> String {
    let indent = "  ".repeat(level);
    s.lines()
        .map(|line| format!("{}{}", indent, line))
        .collect::<Vec<String>>()
        .join("\n")
}

fn generate_dependency_map(
    variable_collection: &HashMap<String, TfVariable>,
    output_collection: &HashMap<String, TfOutput>,
) -> HashMap<String, String> {
    let mut dependency_map = HashMap::new();

    for (key, value) in variable_collection {
        if value.default.is_none() {
            continue;
        }
        let serialized_value = serde_json::to_string(&value.default.clone().unwrap()).unwrap();
        // if variable anywhere matches {{ ModuleName::DeploymentName::OutputName }}, check for output references and insert into dependency_map
        let re = Regex::new(r"(.*?)\{\{\s*(.*?)\s*\}\}(.*)").unwrap();
        for caps in re.captures_iter(serialized_value.as_str()) {
            let before_expr = &caps[1]; // Text before {{ }}
            let expr = &caps[2];        // The inner expression inside {{ }}
            let after_expr = &caps[3];  // Text after {{ }}

            let parts: Vec<&str> = expr.split("::").collect();
            if parts.len() == 3 {
                let _kind = parts[0];
                let claim_name = parts[1];
                let field = parts[2];

                // field in claim: bucketName, in module input/output: bucket_name
                let field_snake_case = to_snake_case(field);
                let output_key = get_output_name(claim_name, &field_snake_case);
                let variable_key = key.to_string();

                if output_collection.contains_key(&output_key) {
                    let full_output_key = format!("{}${{module.{}.{}}}{}", before_expr, to_snake_case(claim_name), field_snake_case, after_expr);
                    dependency_map.insert(variable_key, full_output_key);
                } else if variable_collection.contains_key(&output_key) {
                    // check if variable is variables, if so use directly
                    let full_output_key = format!("{}${{var.{}}}{}", before_expr, get_variable_name(claim_name, &field_snake_case), after_expr);
                    dependency_map.insert(variable_key, full_output_key);
                } else {
                    panic!("Output key not found in outputs: {}", output_key);
                }
            }
        }
    }

    dependency_map
}

fn collect_modules(
    claim_modules: &Vec<(DeploymentManifest, ModuleResp)>,
) -> HashMap<String, ModuleResp> {
    let mut modules = HashMap::new();

    for (claim, module) in claim_modules {
        modules.insert(to_snake_case(&claim.metadata.name), module.clone());
    }

    modules
}

fn get_variable_name(claim_name: &str, variable_name: &str) -> String {
    format!("{}__{}", to_snake_case(claim_name), variable_name)
}

fn get_output_name(claim_name: &str, output_name: &str) -> String {
    format!("{}__{}", to_snake_case(claim_name), output_name)
}

fn collect_module_outputs(
    claim_modules: &Vec<(DeploymentManifest, ModuleResp)>,
) -> HashMap<String, TfOutput> {
    let mut outputs = HashMap::new();

    for (claim, module) in claim_modules {
        for output in &module.tf_outputs {
            let output_key = get_output_name(&claim.metadata.name, &output.name);
            outputs.insert(output_key, output.clone());
        }
    }

    outputs
}

// Create list of all variables from all modules
fn collect_module_variables(
    claim_modules: &Vec<(DeploymentManifest, ModuleResp)>,
) -> HashMap<String, TfVariable> {
    let mut variables = HashMap::new();

    for (claim, module) in claim_modules {
        let claim_variables = &claim.spec.variables;
        for tf_var in &module.tf_variables {
            let var_name = get_variable_name(&claim.metadata.name, &tf_var.name);

            // In claim: bucketName, in module: bucket_name
            let camelcase_var_name = to_camel_case(&tf_var.name);
            let new_tf_var =
                match claim_variables.get(&serde_yaml::Value::String(camelcase_var_name)) {
                    Some(value) => {
                        // Variable defined in claim, use claim value
                        let mut temp_tf_var = tf_var.clone();
                        temp_tf_var.default = Some(serde_json::to_value(value).unwrap());
                        temp_tf_var
                    }
                    None => tf_var.clone(),
                };

            variables.insert(var_name, new_tf_var);
        }
    }

    variables
}

pub async fn publish_stack(
    manifest_path: &String,
    environment: &String,
) -> anyhow::Result<(), anyhow::Error> {
    println!("Publishing stack from {}", manifest_path);
    let stack_manifest = get_stack_manifest(manifest_path).await;
    let claims = get_claims_in_stack(manifest_path).await;
    let claim_modules = get_modules_in_stack(&claims).await;

    let (modules_str, variables_str, outputs_str) = generate_full_terraform_module(&claim_modules);

    let tf_variables = get_variables_from_tf_files(&variables_str).unwrap();
    let tf_outputs = get_outputs_from_tf_files(&outputs_str).unwrap();

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

    let mut zip_parts: HashMap<String, Vec<u8>> = HashMap::new();

    let main_module_zip = merge_zips(env_utils::ZipInput::WithoutFolders(
        vec![
            get_zip_file_from_str(&modules_str, "main.tf").unwrap(),
            get_zip_file_from_str(&variables_str, "variables.tf").unwrap(),
            get_zip_file_from_str(&outputs_str, "outputs.tf").unwrap(),
        ]
    )).unwrap();

    zip_parts.insert("./".to_string(), main_module_zip); // Add main module files zip to root

    // Download any additional modules that are used in the stack and bundle with module zip
    if let Some(module_stack_data) = &module.stack_data {
        for stack_module in &module_stack_data.modules {
            let module_zip: Vec<u8> = download_module_to_vec(&stack_module.s3_key).await;
            let (_module_name, file_name) = stack_module.s3_key.split_once('/').unwrap();
            let folder_name = file_name.trim_end_matches(".zip").to_string();
            zip_parts.insert( folder_name, module_zip);
        }
    }

    let full_zip = merge_zips(env_utils::ZipInput::WithFolders(zip_parts)).unwrap();

    let zip_base64 = base64::encode(&full_zip);

    println!("Uploading stack as module {}", &module.module);
    crate::api_module::upload_module(&module, &zip_base64, &environment).await
}

async fn get_claims_in_stack(manifest_path: &String) -> Vec<DeploymentManifest> {
    println!("Reading stack claim manifests in {}", manifest_path);
    let claims =
        read_stack_directory(Path::new(manifest_path)).expect("Failed to read stack directory");
    claims
}

async fn download_module_to_vec(
    s3_key: &String,
) -> Vec<u8> {
    println!("Downloading module from {}...", s3_key);

    let url = match get_module_download_url(s3_key).await {
        Ok(url) => url,
        Err(e) => {
            panic!("Error: {:?}", e);
        }
    };

    let zip_vec = match env_utils::download_zip_to_vec(&url).await {
        Ok(content) => {
            println!("Downloaded module");
            content
        }
        Err(e) => {
            panic!("Error: {:?}", e);
        }
    };

    zip_vec
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

pub async fn get_stack_version(
    module: &String,
    environment: &String,
    version: &String,
) -> anyhow::Result<ModuleResp> {
    crate::api_module::get_module_version(module, environment, version).await
}

pub async fn get_all_stack_versions(
    module: &str,
    environment: &str,
) -> Result<Vec<ModuleResp>, anyhow::Error> {
    crate::api_module::get_all_module_versions(module, environment).await
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use env_defs::{Metadata, ModuleSpec};
    use pretty_assertions::assert_eq;
    use serde_json::{json, Value};


    #[test]
    fn test_snake_case_conversion() {
        assert_eq!(to_snake_case("bucketName"), "bucket_name");
        assert_eq!(to_snake_case("BucketName"), "bucket_name");
        assert_eq!(to_snake_case("bucket1a"), "bucket_1a");
        assert_eq!(to_snake_case("bucket2"), "bucket_2");
        assert_eq!(to_camel_case("bucket_name"), "bucketName");
    }

    #[test]
    fn test_collect_module_variables() {
        let claim_modules = get_example_claim_modules();

        let generated_variable_collection = collect_module_variables(&claim_modules);

        let expected_variable_collection = {
            let mut map = BTreeMap::new();

            map.extend([
                (
                    "bucket_2__bucket_name".to_string(),
                    TfVariable {
                        name: "bucket_name".to_string(),
                        default: Some(Value::String(
                            "{{ S3Bucket::bucket1a::bucketName }}-after".to_string(),
                        )),
                        _type: Value::String("string".to_string()),
                        description: Some("Name of the S3 bucket".to_string()),
                        nullable: Some(false),
                        sensitive: Some(false),
                    },
                ),
                (
                    "bucket_2__tags".to_string(),
                    TfVariable {
                        name: "tags".to_string(),
                        default: Some(
                            serde_json::to_value(json!(
                            {
                                "Name234": "my-s3bucket-bucket2",
                                "dependentOn": "prefix-{{ S3Bucket::bucket1a::bucketArn }}-suffix"
                            }
                            ))
                            .unwrap(),
                        ),
                        _type: Value::String("map(string)".to_string()),
                        description: Some("Tags to apply to the S3 bucket".to_string()),
                        nullable: Some(true),
                        sensitive: Some(false),
                    },
                ),
                (
                    "bucket_1a__bucket_name".to_string(),
                    TfVariable {
                        name: "bucket_name".to_string(),
                        default: Some(Value::Null),
                        _type: Value::String("string".to_string()),
                        description: Some("Name of the S3 bucket".to_string()),
                        nullable: Some(false),
                        sensitive: Some(false),
                    },
                ),
                (
                    "bucket_1a__tags".to_string(),
                    TfVariable {
                        name: "tags".to_string(),
                        default: Some(
                            serde_json::to_value(json!(
                                {
                                    "Test": "hej",
                                    "AnotherTag": "something"
                                }
                            ))
                            .unwrap(),
                        ),
                        _type: Value::String("map(string)".to_string()),
                        description: Some("Tags to apply to the S3 bucket".to_string()),
                        nullable: Some(true),
                        sensitive: Some(false),
                    },
                ),
            ]);
            map
        };

        // Convert generated_variable_collection to BTreeMap for consistent ordering
        let generated_variable_collection: BTreeMap<_, _> =
            generated_variable_collection.into_iter().collect();

        assert_eq!(
            serde_json::to_string_pretty(&generated_variable_collection).unwrap(),
            serde_json::to_string_pretty(&expected_variable_collection).unwrap()
        );
    }

    #[test]
    fn test_collect_module_outputs() {
        let claim_modules = get_example_claim_modules();

        // Call the function under test
        let generated_output_collection = collect_module_outputs(&claim_modules);

        let expected_output_collection = {
            let mut map = BTreeMap::new();

            map.extend([
                (
                    "bucket_2__bucket_arn".to_string(),
                    TfOutput {
                        name: "bucket_arn".to_string(),
                        value: "".to_string(),
                        description: "ARN of the bucket".to_string(),
                    },
                ),
                (
                    "bucket_1a__bucket_arn".to_string(),
                    TfOutput {
                        name: "bucket_arn".to_string(),
                        value: "".to_string(),
                        description: "ARN of the bucket".to_string(),
                    },
                ),
            ]);
            map
        };

        // Convert generated_output_collection to BTreeMap for consistent ordering
        let generated_output_collection: BTreeMap<_, _> =
            generated_output_collection.into_iter().collect();

        assert_eq!(
            serde_json::to_string_pretty(&generated_output_collection).unwrap(),
            serde_json::to_string_pretty(&expected_output_collection).unwrap()
        );
    }

    #[test]
    fn test_generate_dependency_map() {
        let claim_modules = get_example_claim_modules();

        let generated_variable_collection = collect_module_variables(&claim_modules);
        let generated_output_collection = collect_module_outputs(&claim_modules);

        // Call the function under test
        let generated_dependency_map =
            generate_dependency_map(&generated_variable_collection, &generated_output_collection);

        let expected_dependency_map = {
            let mut map = HashMap::new();
            map.insert(
                "bucket_2__bucket_name".to_string(),
                "\"${var.bucket_1a__bucket_name}-after\"".to_string(),
            );
            map.insert(
                "bucket_2__tags".to_string(),
                "{\"Name234\":\"my-s3bucket-bucket2\",\"dependentOn\":\"prefix-${module.bucket_1a.bucket_arn}-suffix\"}".to_string(),
            );
            map
        };

        assert_eq!(generated_dependency_map, expected_dependency_map);
    }

    #[test]
    fn test_generate_terraform_variables() {
        let claim_modules = get_example_claim_modules();

        let generated_variable_collection = collect_module_variables(&claim_modules);
        let generated_output_collection = collect_module_outputs(&claim_modules);

        let generated_dependency_map =
            generate_dependency_map(&generated_variable_collection, &generated_output_collection);
            println!("{:?}", generated_dependency_map);

        // Call the function under test
        let generated_terraform_variables_string = generate_terraform_variables(
            &generated_variable_collection,
            &generated_dependency_map,
        );

        let expected_terraform_variables_string = r#"
variable "bucket_1a__bucket_name" {
  type = string
  default = null
}

variable "bucket_1a__tags" {
  type = map(string)
  default = {
    AnotherTag = "something"
    Test = "hej"
  }
}"#;

        assert_eq!(
            generated_terraform_variables_string,
            expected_terraform_variables_string
        );
    }


    #[test]
    fn test_generate_terraform_outputs() {
        let claim_modules = get_example_claim_modules();

        let generated_variable_collection = collect_module_variables(&claim_modules);
        let generated_output_collection = collect_module_outputs(&claim_modules);

        let generated_dependency_map =
            generate_dependency_map(&generated_variable_collection, &generated_output_collection);
            println!("{:?}", generated_dependency_map);

        // Call the function under test
        let generated_terraform_outputs_string = generate_terraform_outputs(
            &generated_output_collection,
            &generated_dependency_map,
        );

        let expected_terraform_outputs_string = r#"
output "bucket_1a__bucket_arn" {
  value = module.bucket_1a.bucket_arn
}

output "bucket_2__bucket_arn" {
  value = module.bucket_2.bucket_arn
}"#;

        assert_eq!(
            generated_terraform_outputs_string,
            expected_terraform_outputs_string
        );
    }


    #[test]
    fn test_generate_terraform_modules() {
        let claim_modules = get_example_claim_modules();

        let generated_variable_collection = collect_module_variables(&claim_modules);
        let generated_module_collection = collect_modules(&claim_modules);
        let generated_output_collection = collect_module_outputs(&claim_modules);

        let generated_dependency_map =
            generate_dependency_map(&generated_variable_collection, &generated_output_collection);
        
        println!("{:?}", generated_module_collection);

        // Call the function under test
        let generated_terraform_outputs_string = generate_terraform_modules(
            &generated_module_collection,
            &generated_variable_collection,
            &generated_dependency_map,
        );

        let expected_terraform_outputs_string = r#"
module "bucket_1a" {
  source = "./s3bucket-0.0.21"

  bucket_name = var.bucket_1a__bucket_name
  tags = var.bucket_1a__tags
}

module "bucket_2" {
  source = "./s3bucket-0.0.21"

  bucket_name = "${var.bucket_1a__bucket_name}-after"
  tags = {
    Name234 = "my-s3bucket-bucket2"
    dependentOn = "prefix-${module.bucket_1a.bucket_arn}-suffix"
  }
}"#;

        assert_eq!(
            generated_terraform_outputs_string,
            expected_terraform_outputs_string
        );
    }


    #[test]
    fn test_generate_full_terraform_module() {
        let claim_modules = get_example_claim_modules();

        // Call the function under test
        let (modules_str, variables_str, outputs_str) = generate_full_terraform_module(&claim_modules);
        let generated_terraform_module = format!(
            "{}\n{}\n{}",
            modules_str, variables_str, outputs_str
        );

        let expected_terraform_module = r#"
module "bucket_1a" {
  source = "./s3bucket-0.0.21"

  bucket_name = var.bucket_1a__bucket_name
  tags = var.bucket_1a__tags
}

module "bucket_2" {
  source = "./s3bucket-0.0.21"

  bucket_name = "${var.bucket_1a__bucket_name}-after"
  tags = {
    Name234 = "my-s3bucket-bucket2"
    dependentOn = "prefix-${module.bucket_1a.bucket_arn}-suffix"
  }
}

variable "bucket_1a__bucket_name" {
  type = string
  default = null
}

variable "bucket_1a__tags" {
  type = map(string)
  default = {
    AnotherTag = "something"
    Test = "hej"
  }
}

output "bucket_1a__bucket_arn" {
  value = module.bucket_1a.bucket_arn
}

output "bucket_2__bucket_arn" {
  value = module.bucket_2.bucket_arn
}"#;

        assert_eq!(
            generated_terraform_module,
            expected_terraform_module
        );
    }

    fn get_example_claim_modules() -> Vec<(DeploymentManifest, ModuleResp)> {
        let yaml_manifest_bucket1a = r#"
    apiVersion: infrabridge.io/v1
    kind: S3Bucket
    metadata:
        name: bucket1a
    spec:
        moduleVersion: 0.0.21
        variables: {}
    "#;

        let yaml_manifest_bucket2 = r#"
    apiVersion: infrabridge.io/v1
    kind: S3Bucket
    metadata:
        name: bucket2
    spec:
        moduleVersion: 0.0.21
        variables:
            bucketName: "{{ S3Bucket::bucket1a::bucketName }}-after"
            tags:
                Name234: my-s3bucket-bucket2
                dependentOn: "prefix-{{ S3Bucket::bucket1a::bucketArn }}-suffix"
    "#;

        // Parse the YAML manifests into DeploymentManifest structures
        let deployment_manifest_bucket1a: DeploymentManifest =
            serde_yaml::from_str(yaml_manifest_bucket1a).unwrap();
        let deployment_manifest_bucket2: DeploymentManifest =
            serde_yaml::from_str(yaml_manifest_bucket2).unwrap();

        // Define ModuleResp instances for each manifest
        let s3bucket_module = s3bucket_module();

        // Create a vector of (DeploymentManifest, ModuleResp) tuples
        let claim_modules = vec![
            (
                deployment_manifest_bucket1a.clone(),
                s3bucket_module.clone(),
            ),
            (deployment_manifest_bucket2.clone(), s3bucket_module.clone()),
        ];
        claim_modules
    }

    fn s3bucket_module() -> ModuleResp {
        ModuleResp {
            s3_key: "s3bucket/s3bucket-0.0.21.zip".to_string(),
            environment: "dev".to_string(),
            environment_version: "dev#000.000.021".to_string(),
            version: "0.0.21".to_string(),
            timestamp: "2024-10-10T22:23:14.368+02:00".to_string(),
            module_name: "S3Bucket".to_string(),
            module: "s3bucket".to_string(),
            description: "Some description...".to_string(),
            reference: "https://github.com/infreweave-io/modules/s3bucket".to_string(),
            manifest: ModuleManifest {
                metadata: Metadata {
                    name: "metadata".to_string(),
                },
                api_version: "infrabridge.io/v1".to_string(),
                kind: "Module".to_string(),
                spec: ModuleSpec {
                    module_name: "S3Bucket".to_string(),
                    version: "0.0.21".to_string(),
                    description: "Some description...".to_string(),
                    reference: "https://github.com/infreweave-io/modules/s3bucket".to_string(),
                },
            },
            stack_data: None,
            tf_outputs: vec![
                TfOutput {
                    name: "bucket_arn".to_string(),
                    description: "ARN of the bucket".to_string(),
                    value: "".to_string(),
                },
                // TfOutput { name: "region".to_string(), description: "".to_string(), value: "".to_string() },
                // TfOutput { name: "sse_algorithm".to_string(), description: "".to_string(), value: "".to_string() },
            ],
            tf_variables: vec![
                TfVariable {
                    default: None,
                    name: "bucket_name".to_string(),
                    description: Some("Name of the S3 bucket".to_string()),
                    _type: Value::String("string".to_string()),
                    nullable: Some(false),
                    sensitive: Some(false),
                },
                // TfVariable { default: None, name: "enable_acl".to_string(), description: Some("Enable ACL for the S3 bucket".to_string()), _type: Value::Bool(false), nullable: Some(false), sensitive: Some(false) },
                TfVariable {
                    _type: Value::String("map(string)".to_string()),
                    name: "tags".to_string(),
                    description: Some("Tags to apply to the S3 bucket".to_string()),
                    default: serde_json::from_value(serde_json::json!({"Test": "hej", "AnotherTag": "something"})).unwrap(),
                    nullable: Some(true),
                    sensitive: Some(false),
                },
            ],
        }
    }
}
