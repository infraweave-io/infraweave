use env_defs::{
    CloudProvider, DeploymentManifest, ModuleExample, ModuleManifest, ModuleResp,
    ModuleVersionDiff, StackManifest, TfLockProvider, TfOutput, TfRequiredProvider, TfVariable,
};
use env_utils::{
    get_outputs_from_tf_files, get_timestamp, get_variables_from_tf_files, get_version_track,
    get_zip_file_from_str, indent, merge_zips, read_stack_directory, semver_parse, to_camel_case,
    to_snake_case, zero_pad_semver,
};
use hcl::Value as HclValue;
use log::info;
use regex::Regex;
use serde_json::Value as JsonValue;
use std::{
    collections::{HashMap, HashSet},
    path::Path,
};

use crate::{
    errors::ModuleError,
    interface::GenericCloudHandler,
    logic::{
        api_infra::{get_default_cpu, get_default_memory},
        api_module::{compare_latest_version, download_module_to_vec, upload_module},
        utils::{ensure_track_matches_version, ModuleType},
    },
};

pub async fn publish_stack(
    handler: &GenericCloudHandler,
    manifest_path: &str,
    track: &str,
    version_arg: Option<&str>,
) -> anyhow::Result<(), ModuleError> {
    println!("Publishing stack from {}", manifest_path);

    let mut stack_manifest = get_stack_manifest(manifest_path);

    validate_stack_name(&stack_manifest)?;

    if version_arg.is_some() {
        // In case a version argument is provided
        if stack_manifest.spec.version.is_some() {
            panic!("Version is not allowed when version is already set in module.yaml");
        }
        info!("Using version: {}", version_arg.as_ref().unwrap());
        stack_manifest.spec.version = Some(version_arg.unwrap().to_string());
    }
    let claims = get_claims_in_stack(manifest_path)?;
    let claim_modules = get_modules_in_stack(handler, &claims).await;

    validate_claim_modules(&claim_modules)?;

    let (modules_str, variables_str, outputs_str, providers) =
        generate_full_terraform_module(&claim_modules)?;

    let tf_variables = get_variables_from_tf_files(&variables_str).unwrap();
    let tf_outputs = get_outputs_from_tf_files(&outputs_str).unwrap();
    let tf_required_providers = providers.clone();
    let tf_lock_providers = providers
        .iter()
        .map(|p| TfLockProvider {
            source: p.source.clone(),
            version: p.version.clone(),
        })
        .collect::<Vec<_>>();

    let module = stack_manifest.metadata.name.clone();
    let version = match stack_manifest.spec.version.clone() {
        Some(version) => version,
        None => {
            return Err(ModuleError::ModuleVersionMissing(
                stack_manifest.metadata.name.clone(),
            ));
        }
    };

    let stack_manifest_clone = stack_manifest.clone();

    validate_examples(&tf_variables, &mut stack_manifest.spec.examples)?;

    let module_manifest = ModuleManifest {
        metadata: env_defs::Metadata {
            name: stack_manifest.metadata.name.clone(),
        },
        kind: stack_manifest.kind.clone(),
        spec: env_defs::ModuleSpec {
            module_name: stack_manifest.spec.stack_name.clone(),
            version: Some(version.clone()),
            description: stack_manifest.spec.description.clone(),
            reference: stack_manifest.spec.reference.clone(),
            examples: stack_manifest.spec.examples.clone(),
            cpu: Some(
                stack_manifest_clone
                    .spec
                    .cpu
                    .unwrap_or_else(get_default_cpu),
            ),
            memory: Some(
                stack_manifest_clone
                    .spec
                    .memory
                    .unwrap_or_else(get_default_memory),
            ),
        },
        api_version: stack_manifest.api_version.clone(),
    };

    let stack_data = Some(env_defs::ModuleStackData {
        modules: claim_modules
            .iter()
            .map(|(_d, m)| env_defs::StackModule {
                module: m.module.clone(),
                version: m.version.clone(),
                track: m.track.clone(),
                s3_key: m.s3_key.clone(),
            })
            .collect(),
    });

    ensure_track_matches_version(track, &version)?;

    let latest_version: Option<ModuleResp> =
        match compare_latest_version(handler, &module, &version, track, ModuleType::Module).await {
            Ok(existing_version) => existing_version, // Returns existing module if newer, otherwise it's the first module version to be published
            Err(error) => {
                println!("{}", error);
                std::process::exit(1); // If the module version already exists and is older, exit
            }
        };

    let tf_content = format!("{}\n{}\n{}", &modules_str, &variables_str, &outputs_str);

    let version_diff = match latest_version {
        Some(previous_existing_module) => {
            let current_version_module_hcl_str = &tf_content;

            // Download the previous version of the module and get hcl content
            let previous_version_s3_key = &previous_existing_module.version;
            let previous_version_module_zip =
                download_module_to_vec(handler, previous_version_s3_key).await;

            // Extract all hcl blocks from the zip file
            let previous_version_module_hcl_str =
                match env_utils::read_tf_from_zip(&previous_version_module_zip) {
                    Ok(hcl_str) => hcl_str,
                    Err(error) => {
                        println!("{}", error);
                        std::process::exit(1);
                    }
                };

            // Compare with existing hcl blocks in current version
            let (additions, changes, deletions) = env_utils::diff_modules(
                current_version_module_hcl_str,
                &previous_version_module_hcl_str,
            );

            Some(ModuleVersionDiff {
                added: additions,
                changed: changes,
                removed: deletions,
                previous_version: previous_existing_module.version.clone(),
            })
        }
        None => None,
    };

    let stack_manifest_clone = stack_manifest.clone();
    let cpu = stack_manifest_clone
        .spec
        .cpu
        .as_ref()
        .unwrap_or(&get_default_cpu())
        .to_string();
    let memory = stack_manifest_clone
        .spec
        .memory
        .as_ref()
        .unwrap_or(&get_default_memory())
        .to_string();

    let module = ModuleResp {
        track: track.to_string(),
        track_version: format!(
            "{}#{}",
            track,
            zero_pad_semver(version.as_str(), 3).map_err(|e| anyhow::anyhow!(e))?
        ),
        version: version.clone(),
        timestamp: get_timestamp(),
        module: stack_manifest.metadata.name.clone(),
        module_name: stack_manifest.spec.stack_name.clone(),
        module_type: "stack".to_string(),
        description: stack_manifest.spec.description.clone(),
        reference: stack_manifest.spec.reference.clone(),
        manifest: module_manifest,
        tf_variables,
        tf_outputs,
        tf_required_providers,
        tf_lock_providers,
        s3_key: format!(
            "{}/{}-{}.zip",
            &stack_manifest.metadata.name, &stack_manifest.metadata.name, &version
        ), // s3_key -> "{module}/{module}-{version}.zip"
        stack_data,
        version_diff,
        cpu: cpu.clone(),
        memory: memory.clone(),
    };

    let mut zip_parts: HashMap<String, Vec<u8>> = HashMap::new();

    let main_module_zip = merge_zips(env_utils::ZipInput::WithoutFolders(vec![
        get_zip_file_from_str(&modules_str, "main.tf").map_err(|e| anyhow::anyhow!(e))?,
        get_zip_file_from_str(&variables_str, "variables.tf").map_err(|e| anyhow::anyhow!(e))?,
        get_zip_file_from_str(&outputs_str, "outputs.tf").map_err(|e| anyhow::anyhow!(e))?,
    ]))
    .map_err(|e| anyhow::anyhow!("{}", e))?;

    zip_parts.insert("./".to_string(), main_module_zip); // Add main module files zip to root

    // Download any additional modules that are used in the stack and bundle with module zip
    if let Some(module_stack_data) = &module.stack_data {
        for stack_module in &module_stack_data.modules {
            let module_zip: Vec<u8> = download_module_to_vec(handler, &stack_module.s3_key).await;
            let (_module_name, file_name) = stack_module.s3_key.split_once('/').unwrap();
            let folder_name = file_name.trim_end_matches(".zip").to_string();
            zip_parts.insert(folder_name, module_zip);
        }
    }

    let full_zip = merge_zips(env_utils::ZipInput::WithFolders(zip_parts)).unwrap();
    let zip_base64 = base64::encode(&full_zip);

    match compare_latest_version(
        handler,
        &module.module,
        &module.version,
        track,
        ModuleType::Stack,
    )
    .await
    {
        Ok(_) => (),
        Err(error) => {
            println!("{}", error);
            std::process::exit(1);
        }
    }

    let all_regions = handler.get_all_regions().await?;
    info!("Publishing stack in all regions: {:?}", all_regions);
    for region in all_regions {
        let region_handler = handler.copy_with_region(&region).await;
        match upload_module(&region_handler, &module, &zip_base64).await {
            Ok(_) => {
                info!("Stack published successfully in region {}", region);
            }
            Err(error) => {
                return Err(ModuleError::UploadModuleError(error.to_string()));
            }
        }
    }

    info!("Stack published successfully in all regions!");
    Ok(())
}

fn validate_stack_name(stack_manifest: &StackManifest) -> anyhow::Result<(), ModuleError> {
    let name = stack_manifest.metadata.name.clone();
    let stack_name = stack_manifest.spec.stack_name.clone();
    let re = Regex::new(r"^[a-z][a-z0-9]+$").unwrap();
    if !re.is_match(&name) {
        return Err(ModuleError::ValidationError(format!(
            "The name {} must only use lowercase characters and numbers.",
            name,
        )));
    }
    if stack_name.to_lowercase() != name {
        return Err(ModuleError::ValidationError(format!(
            "The name {} must exactly match lowercase of the stackName specified under spec {}.",
            name, stack_name
        )));
    }
    Ok(())
}

pub async fn get_stack_preview(
    handler: &GenericCloudHandler,
    manifest_path: &String,
) -> anyhow::Result<String, anyhow::Error> {
    println!("Preview stack from {}", manifest_path);

    let claims = get_claims_in_stack(manifest_path)?;
    let claim_modules = get_modules_in_stack(handler, &claims).await;

    let (modules_str, variables_str, outputs_str, _providers) =
        generate_full_terraform_module(&claim_modules)?;

    let tf_content = format!("{}\n{}\n{}", &modules_str, &variables_str, &outputs_str);

    Ok(tf_content)
}

fn get_stack_manifest(manifest_path: &str) -> StackManifest {
    println!("Reading stack manifest in {}", manifest_path);
    let stack_yaml_path = Path::new(manifest_path).join("stack.yaml");
    let manifest =
        std::fs::read_to_string(&stack_yaml_path).expect("Failed to read stack manifest file");

    serde_yaml::from_str::<StackManifest>(&manifest).expect("Failed to parse stack manifest")
}

fn get_claims_in_stack(manifest_path: &str) -> Result<Vec<DeploymentManifest>, anyhow::Error> {
    println!("Reading stack claim manifests in {}", manifest_path);
    let claims = read_stack_directory(Path::new(manifest_path))?;
    Ok(claims)
}

async fn get_modules_in_stack(
    handler: &GenericCloudHandler,
    deployment_manifests: &Vec<DeploymentManifest>,
) -> Vec<(DeploymentManifest, ModuleResp)> {
    println!("Getting modules for deployment manifests");
    let mut claim_modules: Vec<(DeploymentManifest, ModuleResp)> = vec![];

    for claim in deployment_manifests {
        let module_version = match &claim.spec.module_version {
            Some(version) => version, // We expect module version to be set for all claims
            None => {
                println!("Module version is not set in claim {}", claim.metadata.name);
                std::process::exit(1); // TODO: should propagate error up instead of exiting
            }
        };
        assert_eq!(claim.spec.stack_version, None); // Stack version should not be set in claims
        let track = match get_version_track(module_version) {
            Ok(track) => track,
            Err(e) => {
                println!(
                    "Could not find track for claim {}, error: {}",
                    claim.metadata.name, e
                );
                std::process::exit(1); // TODO: should propagate error up instead of exiting
            }
        };
        let module = claim.kind.to_lowercase();
        let version = module_version.to_string();
        let module_resp = match handler.get_module_version(&module, &track, &version).await {
            Ok(result) => match result {
                Some(m) => m,
                None => {
                    println!(
                        "No module found with name: {} and version: {}",
                        &module, &version
                    );
                    std::process::exit(1);
                }
            },
            Err(e) => {
                println!("{}", e);
                std::process::exit(1);
            }
        };
        claim_modules.push((claim.clone(), module_resp));
    }

    claim_modules
}

pub fn generate_full_terraform_module(
    claim_modules: &Vec<(DeploymentManifest, ModuleResp)>,
) -> Result<(String, String, String, Vec<TfRequiredProvider>), ModuleError> {
    let variable_collection = collect_module_variables(claim_modules);
    let output_collection = collect_module_outputs(claim_modules);
    let module_collection = collect_modules(claim_modules);

    // Create list of all dependencies between modules
    // Maps every "{{ ModuleName::DeploymentName::OutputName }}" to the output key such as "module.DeploymentName.OutputName"
    let dependency_map = generate_dependency_map(&variable_collection, &output_collection)?;

    let (terraform_module_code, providers) =
        generate_terraform_modules(&module_collection, &variable_collection, &dependency_map);

    let terraform_variable_code =
        generate_terraform_variables(&variable_collection, &dependency_map);

    let terraform_output_code = generate_terraform_outputs(&output_collection, &dependency_map);

    Ok((
        terraform_module_code,
        terraform_variable_code,
        terraform_output_code,
        providers,
    ))
}
fn generate_terraform_block(
    modules: &HashMap<String, ModuleResp>,
) -> (String, Vec<TfRequiredProvider>) {
    // Pick the latest-version lock for each source
    let latest_locks = modules
        .values()
        .flat_map(|m| m.tf_lock_providers.iter().cloned())
        .fold(HashMap::new(), |mut acc, p| {
            acc.entry(p.source.clone())
                .and_modify(|existing: &mut TfLockProvider| {
                    if semver_parse(&p.version).unwrap() > semver_parse(&existing.version).unwrap()
                    {
                        *existing = p.clone();
                    }
                })
                .or_insert(p);
            acc
        });

    let name_map = modules
        .values()
        .flat_map(|m| {
            m.tf_required_providers
                .iter()
                .map(|rp| (rp.source.clone(), rp.name.clone()))
        })
        .collect::<HashMap<_, _>>();

    let providers = latest_locks
        .into_values()
        .map(|p| TfRequiredProvider {
            source: p.source.clone(),
            name: name_map
                .get(&p.source)
                .expect("missing provider name")
                .clone(),
            version: p.version.clone(),
        })
        .collect::<Vec<_>>();

    let providers_str = providers
        .iter()
        .map(|p| {
            format!(
                "\n  {} = {{\n    source = \"{}\"\n    version = \"{}\"\n  }}",
                name_map.get(&p.source).expect("missing provider name"),
                p.source,
                p.version
            )
        })
        .collect::<String>();

    let terraform_block = format!(
        r#"
terraform {{
  required_providers {{{}
  }}
}}
"#,
        indent(&providers_str, 2)
    );

    (terraform_block, providers)
}

fn generate_terraform_modules(
    module_collection: &HashMap<String, ModuleResp>,
    variable_collection: &HashMap<String, TfVariable>,
    dependency_map: &HashMap<String, String>,
) -> (String, Vec<TfRequiredProvider>) {
    let mut terraform_modules = vec![];

    let (terraform_block_str, providers) = generate_terraform_block(&module_collection);

    for (claim_name, module) in module_collection {
        let module_str = generate_terraform_module_single(
            claim_name,
            module,
            variable_collection,
            dependency_map,
        );
        terraform_modules.push(module_str);
    }

    terraform_modules.sort(); // Sort for consistent ordering
    let tf_block = format!("{}{}", terraform_block_str, terraform_modules.join("\n"));

    (tf_block, providers)
}

fn generate_terraform_module_single(
    claim_name: &str,
    module: &ModuleResp,
    variable_collection: &HashMap<String, TfVariable>,
    dependency_map: &HashMap<String, String>,
) -> String {
    let mut module_str = String::new();
    let source = module
        .s3_key
        .split('/')
        .last()
        .unwrap()
        .trim_end_matches(".zip");
    module_str.push_str(
        format!(
            "\nmodule \"{}\" {{\n  source = \"./{}\"\n",
            to_snake_case(claim_name),
            source,
        )
        .as_str(),
    );

    let variable_collection: std::collections::BTreeMap<_, _> =
        variable_collection.iter().collect(); // Not necessary, but for consistent ordering of variables

    for (variable_name, _variable_value) in variable_collection {
        let parts = variable_name.split("__").collect::<Vec<&str>>();
        let part_claim_name = parts[0];
        let part_var_name = parts[1];

        if part_claim_name != claim_name {
            // Skip if variable is not for this module
            continue;
        }

        if dependency_map.contains_key(variable_name) {
            let dependency_str = dependency_map.get(variable_name).unwrap();
            let variable_str = format!("\n  {} = {}", part_var_name, dependency_str);
            // if can be parses as json, then parse it and print as hcl
            if let Ok(value) = serde_json::from_str(dependency_str) {
                let hcl_value = json_to_hcl(value).to_string();
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
            generate_terraform_output_single(output_name, output_value, dependency_map);
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
            generate_terraform_variable_single(variable_name, variable_value, dependency_map);
        terraform_variables.push(variable_str);
    }

    terraform_variables.sort(); // Sort for consistent ordering
    terraform_variables.join("\n")
}

fn json_to_hcl(value: JsonValue) -> HclValue {
    match value {
        JsonValue::Null => HclValue::Null,
        JsonValue::Bool(b) => HclValue::Bool(b),
        JsonValue::Number(n) => {
            // Try converting to i64 first, then f64
            if let Some(i) = n.as_i64() {
                HclValue::Number(i.into())
            } else if let Some(f) = n.as_f64() {
                HclValue::Number(hcl::Number::from_f64(f).expect("failed to convert float"))
            } else {
                panic!("Unexpected number format")
            }
        }
        JsonValue::String(s) => HclValue::String(s),
        JsonValue::Array(arr) => HclValue::Array(arr.into_iter().map(json_to_hcl).collect()),
        JsonValue::Object(obj) => {
            let hcl_obj: indexmap::IndexMap<_, _> =
                obj.into_iter().map(|(k, v)| (k, json_to_hcl(v))).collect();
            HclValue::Object(hcl_obj)
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
        json_to_hcl(variable.default.clone()).to_string()
    };
    let _type = variable._type.to_string();
    let _type = _type.trim_matches('"'); // remove quotes from type
    let description = variable.description.clone();
    let nullable = variable.nullable;
    let sensitive = variable.sensitive;

    let default_line = if default_value == "null" && !nullable {
        println!("Default value is null and nullable is false for variable {}. This should be added as an example value", var_name);
        "".to_string()
    } else {
        format!("\n{}", indent(&format!("default = {}", &default_value), 1))
    };
    format!(
        r#"
variable "{}" {{
  type = {}{}
  description = "{}"
  nullable = {}
  sensitive = {}
}}"#,
        var_name, _type, &default_line, &description, nullable, sensitive
    )
}

fn generate_dependency_map(
    variable_collection: &HashMap<String, TfVariable>,
    output_collection: &HashMap<String, TfOutput>,
) -> Result<HashMap<String, String>, ModuleError> {
    let mut dependency_map = HashMap::new();

    let re = Regex::new(r"(.*?)\{\{\s*(.*?)\s*\}\}(.*)").unwrap();
    for (key, value) in variable_collection {
        if value.default == serde_json::Value::Null {
            continue;
        }
        let serialized_value = serde_json::to_string(&value.default.clone()).unwrap();
        // if variable anywhere matches {{ ModuleName::DeploymentName::OutputName }}, check for output references and insert into dependency_map
        for caps in re.captures_iter(serialized_value.as_str()) {
            let before_expr = &caps[1]; // Text before {{ }}
            let expr = &caps[2]; // The inner expression inside {{ }}
            let after_expr = &caps[3]; // Text after {{ }}

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
                    let full_output_key = if before_expr == "\"" && after_expr == "\"" {
                        format!("module.{}.{}", to_snake_case(claim_name), field_snake_case)
                    } else {
                        format!(
                            "{}${{module.{}.{}}}{}",
                            before_expr,
                            to_snake_case(claim_name),
                            field_snake_case,
                            after_expr
                        )
                    };
                    dependency_map.insert(variable_key, full_output_key);
                } else if variable_collection.contains_key(&output_key) {
                    // check if variable is variables, if so use directly
                    let full_output_key = if before_expr == "\"" && after_expr == "\"" {
                        format!("var.{}", get_variable_name(claim_name, &field_snake_case))
                    } else {
                        format!(
                            "{}${{var.{}}}{}",
                            before_expr,
                            get_variable_name(claim_name, &field_snake_case),
                            after_expr
                        )
                    };
                    dependency_map.insert(variable_key, full_output_key);
                } else {
                    let source_parts: Vec<&str> = key.split("__").collect();
                    let source_claim = to_camel_case(source_parts[0]);
                    let variable_name = to_camel_case(&value.name);
                    return Err(ModuleError::OutputKeyNotFound(
                        source_claim,
                        variable_name,
                        serialized_value.clone(),
                        field.to_string(),
                        claim_name.to_string(),
                    ));
                }
            }
        }
    }

    Ok(dependency_map)
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
    claim_modules: &[(DeploymentManifest, ModuleResp)],
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
                        temp_tf_var.default = serde_json::to_value(value).unwrap();
                        temp_tf_var
                    }
                    None => tf_var.clone(),
                };

            variables.insert(var_name, new_tf_var);
        }
    }

    variables
}

pub fn validate_claim_modules(
    claim_modules: &[(DeploymentManifest, ModuleResp)],
) -> Result<(), ModuleError> {
    // Ensure unique claim names
    let mut seen = std::collections::HashSet::new();
    let duplicates: Vec<String> = claim_modules
        .iter()
        .map(|(claim, _)| claim.metadata.name.clone())
        .filter(|name| !seen.insert(name.clone()))
        .collect();
    if !duplicates.is_empty() {
        return Err(ModuleError::DuplicateClaimNames(
            duplicates.first().unwrap().clone(),
        ));
    }

    for (claim, module) in claim_modules {
        let deployment_variables: serde_yaml::Mapping = claim.spec.variables.clone();
        let provided_variables: serde_json::Value = if deployment_variables.is_empty() {
            serde_json::json!({})
        } else {
            serde_json::to_value(&deployment_variables).unwrap()
        };
        let variables = env_utils::convert_first_level_keys_to_snake_case(&provided_variables);

        env_utils::verify_variable_claim_casing(claim, &provided_variables)?;

        env_utils::verify_variable_existence_and_type(module, &variables)?;

        // Verify moduleVersion is set
        // TODO: (may support Stacks in future but need more testing)
        if claim.spec.module_version.is_none() {
            return Err(ModuleError::ModuleVersionNotSet(
                claim.metadata.name.clone(),
            ));
        }

        validate_stack_module_claim_name(claim)?;

        validate_stack_module_claim_region_is_na(claim)?;

        // Verify namespace is not set as this is ignored
        if claim.metadata.namespace.is_some() {
            return Err(ModuleError::StackModuleNamespaceIsSet(
                claim.metadata.name.clone(),
            ));
        }
    }

    validate_dependencies(claim_modules)
}

fn validate_stack_module_claim_name(claim: &DeploymentManifest) -> Result<(), ModuleError> {
    let claim_name = &claim.metadata.name;
    let re = Regex::new(r"^[a-z][a-z0-9]+$").unwrap();
    if !re.is_match(&claim_name) {
        return Err(ModuleError::ValidationError(format!(
            "Claim name {} must only use lowercase characters and numbers.",
            claim_name
        )));
    }
    Ok(())
}

fn validate_stack_module_claim_region_is_na(claim: &DeploymentManifest) -> Result<(), ModuleError> {
    let region = &claim.spec.region;
    let claim_name = &claim.metadata.name;
    if region != "N/A" {
        return Err(ModuleError::ValidationError(format!(
            "Claim {} has the region \"{}\" but the value must be set to \"N/A\" when used inside stacks, since this value is overridden by the Stacks region parameter when deployed.",
            claim_name, region
        )));
    }
    Ok(())
}

fn validate_dependencies(
    claim_modules: &[(DeploymentManifest, ModuleResp)],
) -> Result<(), ModuleError> {
    let module_map = build_claim_module_map(claim_modules);

    // Ensure all references are valid
    for (claim, _) in claim_modules {
        let claim_kind = claim.kind.clone();
        let claim_name = claim.metadata.name.clone();
        let vars_json = convert_vars_to_snake_json(&claim.spec.variables);
        for (ref_kind, ref_claim, ref_field) in extract_top_level_deps(&vars_json) {
            if claim_name == ref_claim && claim_kind == ref_kind {
                return Err(ModuleError::SelfReferencingClaim(
                    claim_kind.clone(),
                    claim_name.clone(),
                    to_camel_case(&ref_field),
                ));
            }
            if !claim_reference_exists(&module_map, &ref_kind, &ref_claim, &ref_field) {
                return Err(ModuleError::StackClaimReferenceNotFound(
                    claim.metadata.name.clone(),
                    ref_kind.clone(),
                    ref_claim.clone(),
                    to_camel_case(&ref_field),
                ));
            }
        }
    }

    // Build a dependency graph mapping each claim to the claims it depends on.
    let mut dependency_graph: HashMap<String, Vec<String>> = HashMap::new();

    // Ensure every claim appears in the graph even if it has no outgoing edges.
    for (claim, _) in claim_modules {
        dependency_graph
            .entry(claim.metadata.name.clone())
            .or_default();
    }

    // For each claim, add an edge from it to every claim referenced in its variables.
    for (claim, _) in claim_modules {
        let claim_name = claim.metadata.name.clone();
        let vars_json = convert_vars_to_snake_json(&claim.spec.variables);
        for (_ref_kind, dep_claim, _ref_field) in extract_top_level_deps(&vars_json) {
            if module_map.contains_key(&dep_claim) {
                dependency_graph
                    .entry(claim_name.clone())
                    .or_default()
                    .push(dep_claim);
            }
        }
    }

    // Run cycle detection on the graph.
    if let Some(cycle) = detect_cycle(&dependency_graph) {
        return Err(ModuleError::CircularDependency(cycle));
    }

    Ok(())
}

/// Detects a cycle in the dependency graph.
/// Returns a vector of claim names (in order) forming the cycle if found.
fn detect_cycle(dependency_graph: &HashMap<String, Vec<String>>) -> Option<Vec<String>> {
    // Helper DFS function that returns the cycle path if found.
    fn dfs(
        node: &String,
        graph: &HashMap<String, Vec<String>>,
        visited: &mut HashSet<String>,
        stack: &mut HashSet<String>,
        path: &mut Vec<String>,
    ) -> Option<Vec<String>> {
        if !visited.contains(node) {
            visited.insert(node.clone());
            stack.insert(node.clone());
            path.push(node.clone());

            if let Some(neighbors) = graph.get(node) {
                for neighbor in neighbors {
                    if !visited.contains(neighbor) {
                        if let Some(cycle) = dfs(neighbor, graph, visited, stack, path) {
                            return Some(cycle);
                        }
                    } else if stack.contains(neighbor) {
                        // Cycle found; extract the cycle from the path.
                        if let Some(start_index) = path.iter().position(|n| n == neighbor) {
                            return Some(path[start_index..].to_vec());
                        }
                    }
                }
            }
        }
        stack.remove(node);
        path.pop();
        None
    }

    let mut visited = HashSet::new();
    let mut stack = HashSet::new();
    let mut path = Vec::new();

    for node in dependency_graph.keys() {
        if let Some(cycle) = dfs(node, dependency_graph, &mut visited, &mut stack, &mut path) {
            return Some(cycle);
        }
    }
    None
}

fn build_claim_module_map(
    claim_modules: &[(DeploymentManifest, ModuleResp)],
) -> HashMap<String, &ModuleResp> {
    claim_modules
        .iter()
        .map(|(claim, module)| (claim.metadata.name.clone(), module))
        .collect()
}

/// Turn a YAML Mapping into snake_case JSON for easy string scanning
fn convert_vars_to_snake_json(vars: &serde_yaml::Mapping) -> serde_json::Value {
    let json = if vars.is_empty() {
        serde_json::json!({})
    } else {
        serde_json::to_value(vars).unwrap()
    };
    env_utils::convert_first_level_keys_to_snake_case(&json)
}

/// Find all “{{ Kind::Claim::Field }}” references in each top‑level string
/// plus any nested string inside a top‑level map (one level deep only).
/// Returns a list of (Kind, Claim, Field) tuples.
fn extract_top_level_deps(vars: &serde_json::Value) -> Vec<(String, String, String)> {
    let re = Regex::new(r"\{\{\s*(\w+)::(\w+)::(\w+)\s*\}\}").unwrap();
    let mut deps = Vec::new();

    if let Some(obj) = vars.as_object() {
        for value in obj.values() {
            match value {
                serde_json::Value::String(s) => {
                    for cap in re.captures_iter(s) {
                        deps.push((
                            cap[1].to_string(),
                            cap[2].to_string(),
                            to_snake_case(&cap[3]),
                        ));
                    }
                }
                serde_json::Value::Array(arr) => {
                    for elem in arr {
                        if let serde_json::Value::String(s) = elem {
                            extract_from_str(s, &mut deps, &re);
                        }
                    }
                }
                serde_json::Value::Object(map) => {
                    for nested in map.values() {
                        if let serde_json::Value::String(s) = nested {
                            for cap in re.captures_iter(s) {
                                deps.push((
                                    cap[1].to_string(),
                                    cap[2].to_string(),
                                    to_snake_case(&cap[3]),
                                ));
                            }
                        }
                    }
                }
                _ => {} // Ignore non-referring types (booleans, numbers, null)
            }
        }
    }

    deps
}

fn extract_from_str(s: &str, deps: &mut Vec<(String, String, String)>, re: &Regex) {
    for cap in re.captures_iter(s) {
        deps.push((
            cap[1].to_string(),
            cap[2].to_string(),
            to_snake_case(&cap[3]),
        ));
    }
}

/// Check that the named claim exists and exports the named output or variable
fn claim_reference_exists(
    module_map: &HashMap<String, &ModuleResp>,
    kind_name: &str,
    claim_name: &str,
    field_name: &str,
) -> bool {
    module_map
        .get(claim_name)
        .map(|m| {
            (m.tf_outputs.iter().any(|o| o.name == field_name)
                || m.tf_variables.iter().any(|v| v.name == field_name))
                && m.module_name == kind_name
        })
        .unwrap_or(false)
}

fn to_mapping(value: serde_yaml::Value) -> Option<serde_yaml::Mapping> {
    if let serde_yaml::Value::Mapping(mapping) = value {
        Some(mapping)
    } else {
        None
    }
}

fn is_all_module_example_variables_valid(
    tf_variables: &[TfVariable],
    example_variables: &serde_yaml::Value,
) -> (bool, String) {
    let example_variables = to_mapping(example_variables.clone()).unwrap();
    let top_level_keys = example_variables
        .iter()
        .map(|f| f.0.as_str().unwrap())
        .collect::<HashSet<_>>();
    let variable_top_level_keys = tf_variables
        .iter()
        .map(|f| f.name.as_str().split("__").next().unwrap())
        .collect::<HashSet<_>>();

    // Check if all top level keys in example_variables are present in tf_variables
    for top_level_key in top_level_keys {
        if !variable_top_level_keys.contains(top_level_key) {
            let error = format!(
                "Example variable under claim name {} does not exist",
                top_level_key
            );
            return (false, error);
        }
    }

    let mut required_variables = tf_variables
        .iter()
        .filter(|&x| x.default == serde_json::Value::Null && !x.nullable)
        .collect::<Vec<_>>();

    for (top_level_key, module_variables) in example_variables.iter() {
        let claim_key = top_level_key.as_str().unwrap();

        let module_variables = to_mapping(module_variables.clone()).unwrap();
        for (key, _value) in module_variables.iter() {
            let key_str = key.as_str().unwrap();
            // Check if variable is camelCase
            if key_str != env_utils::to_camel_case(key_str) {
                let error = format!(
                    "Example variable {} is not camelCase like in the deployment claims",
                    key_str
                );
                return (false, error); // Example-variable is not camelCase
            }

            // TODO: Check if variable is hardcoded in claims and report that with more specific error

            let full_variable_name = format!(
                "{}__{}",
                env_utils::to_snake_case(claim_key),
                env_utils::to_snake_case(key_str)
            );
            let tf_variable = tf_variables.iter().find(|&x| x.name == full_variable_name);
            if tf_variable.is_none() {
                let error = format!("Example variable {} does not exist under {} (or maybe it is already set in claim?)", key_str, claim_key);
                return (false, error); // Example-variable does not exist
            }

            // TODO: Check that type is correct

            // Remove found variable
            required_variables.retain(|&x| x.name != full_variable_name);
        }
    }

    if !required_variables.is_empty() {
        if let Some(required_variable) = required_variables.iter().next() {
            let key_str = required_variable.name.split("__").last().unwrap();
            let claim_key = required_variable.name.split("__").next().unwrap();

            let error = format!(
                "Example variable {} under {} is required but is not set",
                key_str, claim_key
            );
            return (false, error); // Required variable is null
        }
    }

    (true, "".to_string())
}

fn validate_examples(
    tf_variables: &Vec<TfVariable>,
    examples: &mut Option<Vec<ModuleExample>>,
) -> Result<(), ModuleError> {
    if let Some(ref mut examples) = examples {
        for example in examples.iter() {
            let example_variables = &example.variables;
            let (is_valid, error) =
                is_all_module_example_variables_valid(tf_variables, example_variables);
            if !is_valid {
                return Err(ModuleError::InvalidExampleVariable(error));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use env_defs::{Metadata, ModuleSpec, TfLockProvider, TfRequiredProvider};
    use pretty_assertions::assert_eq;
    use serde_json::{json, Value};

    #[test]
    fn test_is_example_variables_valid() {
        let tf_variables = vec![
            TfVariable {
                name: "bucket1a__bucket_name".to_string(),
                description: "The name of the bucket".to_string(),
                default: serde_json::Value::Null,
                sensitive: false,
                nullable: false,
                _type: serde_json::Value::String("string".to_string()),
            },
            TfVariable {
                name: "bucket1a__tags".to_string(),
                description: "The tags to apply to the bucket".to_string(),
                default: serde_json::Value::Null,
                sensitive: false,
                nullable: true,
                _type: serde_json::Value::String("map".to_string()),
            },
            TfVariable {
                name: "bucket2__port_mapping".to_string(),
                description: "The port mapping".to_string(),
                default: serde_json::Value::Null,
                sensitive: false,
                nullable: true,
                _type: serde_json::Value::String("list".to_string()),
            },
        ];
        let example_variables = serde_yaml::from_str::<serde_yaml::Value>(
            r#"
            bucket1a:
                bucketName: some-bucket-name
            bucket2:
                portMapping:
                    - port: 80
                      name: http
"#,
        )
        .unwrap();
        let (is_valid, _error) =
            is_all_module_example_variables_valid(&tf_variables, &example_variables);
        assert_eq!(is_valid, true);
    }

    #[test]
    fn test_is_example_variables_invalid_snake_case() {
        let tf_variables = vec![TfVariable {
            name: "bucket1a__bucket_name".to_string(),
            description: "The name of the bucket".to_string(),
            default: serde_json::Value::Null,
            sensitive: false,
            nullable: false,
            _type: serde_json::Value::String("string".to_string()),
        }];
        let example_variables = serde_yaml::from_str::<serde_yaml::Value>(
            r#"
            bucket1a:
                bucket_name: some-bucket-name
"#,
        )
        .unwrap();
        let (is_valid, _error) =
            is_all_module_example_variables_valid(&tf_variables, &example_variables);
        assert_eq!(is_valid, false);
    }

    #[test]
    fn test_is_example_variables_invalid_missing_required() {
        let tf_variables = vec![
            TfVariable {
                name: "bucket1a__bucket_name".to_string(),
                description: "The name of the bucket".to_string(),
                default: serde_json::Value::Null,
                sensitive: false,
                nullable: false,
                _type: serde_json::Value::String("string".to_string()),
            },
            TfVariable {
                name: "bucket1a__tags".to_string(),
                description: "The tags to apply to the bucket".to_string(),
                default: serde_json::Value::Null,
                sensitive: false,
                nullable: true,
                _type: serde_json::Value::String("map".to_string()),
            },
        ];
        let example_variables = serde_yaml::from_str::<serde_yaml::Value>(
            r#"
            bucket1a:
                tags:
                    env: dev
                    department: engineering
"#,
        )
        .unwrap();
        let (is_valid, _error) =
            is_all_module_example_variables_valid(&tf_variables, &example_variables);
        assert_eq!(is_valid, false);
    }

    #[test]
    fn test_snake_case_conversion() {
        assert_eq!(to_snake_case("bucketName"), "bucket_name");
        assert_eq!(to_snake_case("BucketName"), "bucket_name");
        assert_eq!(to_snake_case("bucket1a"), "bucket1a");
        assert_eq!(to_snake_case("bucket2"), "bucket2");
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
                    "bucket2__bucket_name".to_string(),
                    TfVariable {
                        name: "bucket_name".to_string(),
                        default: Value::String(
                            "{{ S3Bucket::bucket1a::bucketName }}-after".to_string(),
                        ),
                        _type: Value::String("string".to_string()),
                        description: "Name of the S3 bucket".to_string(),
                        nullable: false,
                        sensitive: false,
                    },
                ),
                (
                    "bucket2__input_list".to_string(),
                    TfVariable {
                        name: "input_list".to_string(),
                        default: "{{ S3Bucket::bucket1a::listOfStrings }}".into(),
                        _type: Value::String("list(string)".to_string()),
                        description: "Some arbitrary input list".to_string(),
                        nullable: true,
                        sensitive: false,
                    },
                ),
                (
                    "bucket2__tags".to_string(),
                    TfVariable {
                        name: "tags".to_string(),
                        default: serde_json::to_value(json!(
                        {
                            "Name234": "my-s3bucket-bucket2",
                            "dependentOn": "prefix-{{ S3Bucket::bucket1a::bucketArn }}-suffix"
                        }
                        ))
                        .unwrap(),

                        _type: Value::String("map(string)".to_string()),
                        description: "Tags to apply to the S3 bucket".to_string(),
                        nullable: true,
                        sensitive: false,
                    },
                ),
                (
                    "bucket1a__bucket_name".to_string(),
                    TfVariable {
                        name: "bucket_name".to_string(),
                        default: Value::Null,
                        _type: Value::String("string".to_string()),
                        description: "Name of the S3 bucket".to_string(),
                        nullable: false,
                        sensitive: false,
                    },
                ),
                (
                    "bucket1a__input_list".to_string(),
                    TfVariable {
                        name: "input_list".to_string(),
                        default: Value::Null,
                        _type: Value::String("list(string)".to_string()),
                        description: "Some arbitrary input list".to_string(),
                        nullable: true,
                        sensitive: false,
                    },
                ),
                (
                    "bucket1a__tags".to_string(),
                    TfVariable {
                        name: "tags".to_string(),
                        default: serde_json::to_value(json!(
                            {
                                "Test": "hej",
                                "AnotherTag": "something"
                            }
                        ))
                        .unwrap(),

                        _type: Value::String("map(string)".to_string()),
                        description: "Tags to apply to the S3 bucket".to_string(),
                        nullable: true,
                        sensitive: false,
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
                    "bucket2__bucket_arn".to_string(),
                    TfOutput {
                        name: "bucket_arn".to_string(),
                        value: "".to_string(),
                        description: "ARN of the bucket".to_string(),
                    },
                ),
                (
                    "bucket2__list_of_strings".to_string(),
                    TfOutput {
                        name: "list_of_strings".to_string(),
                        value: "".to_string(),
                        description: "Made up list of strings".to_string(),
                    },
                ),
                (
                    "bucket1a__bucket_arn".to_string(),
                    TfOutput {
                        name: "bucket_arn".to_string(),
                        value: "".to_string(),
                        description: "ARN of the bucket".to_string(),
                    },
                ),
                (
                    "bucket1a__list_of_strings".to_string(),
                    TfOutput {
                        name: "list_of_strings".to_string(),
                        value: "".to_string(),
                        description: "Made up list of strings".to_string(),
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
            generate_dependency_map(&generated_variable_collection, &generated_output_collection)
                .unwrap();

        let expected_dependency_map = {
            let mut map = HashMap::new();
            map.insert(
                "bucket2__bucket_name".to_string(),
                "\"${var.bucket1a__bucket_name}-after\"".to_string(),
            );
            map.insert(
                "bucket2__input_list".to_string(),
                "module.bucket1a.list_of_strings".to_string(),
            );
            map.insert(
                "bucket2__tags".to_string(),
                "{\"Name234\":\"my-s3bucket-bucket2\",\"dependentOn\":\"prefix-${module.bucket1a.bucket_arn}-suffix\"}".to_string(),
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
            generate_dependency_map(&generated_variable_collection, &generated_output_collection)
                .unwrap();
        println!("{:?}", generated_dependency_map);

        // Call the function under test
        let generated_terraform_variables_string =
            generate_terraform_variables(&generated_variable_collection, &generated_dependency_map);

        let expected_terraform_variables_string = r#"
variable "bucket1a__bucket_name" {
  type = string
  description = "Name of the S3 bucket"
  nullable = false
  sensitive = false
}

variable "bucket1a__input_list" {
  type = list(string)
  default = null
  description = "Some arbitrary input list"
  nullable = true
  sensitive = false
}

variable "bucket1a__tags" {
  type = map(string)
  default = {
    "AnotherTag" = "something"
    "Test" = "hej"
  }
  description = "Tags to apply to the S3 bucket"
  nullable = true
  sensitive = false
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
            generate_dependency_map(&generated_variable_collection, &generated_output_collection)
                .unwrap();
        println!("{:?}", generated_dependency_map);

        // Call the function under test
        let generated_terraform_outputs_string =
            generate_terraform_outputs(&generated_output_collection, &generated_dependency_map);

        let expected_terraform_outputs_string = r#"
output "bucket1a__bucket_arn" {
  value = module.bucket1a.bucket_arn
}

output "bucket1a__list_of_strings" {
  value = module.bucket1a.list_of_strings
}

output "bucket2__bucket_arn" {
  value = module.bucket2.bucket_arn
}

output "bucket2__list_of_strings" {
  value = module.bucket2.list_of_strings
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
            generate_dependency_map(&generated_variable_collection, &generated_output_collection)
                .unwrap();

        println!("{:?}", generated_module_collection);

        // Call the function under test
        let (generated_terraform_outputs_string, _providers) = generate_terraform_modules(
            &generated_module_collection,
            &generated_variable_collection,
            &generated_dependency_map,
        );

        // Two versions exist (5.81.0 and 5.95.0), ensure the latest is used
        let expected_terraform_outputs_string = r#"
terraform {
  required_providers {    
      aws = {
        source = "hashicorp/aws"
        version = "5.95.0"
      }
  }
}

module "bucket1a" {
  source = "./s3bucket-0.0.21"

  bucket_name = var.bucket1a__bucket_name
  input_list = var.bucket1a__input_list
  tags = var.bucket1a__tags
}

module "bucket2" {
  source = "./s3bucket-0.0.22"

  bucket_name = "${var.bucket1a__bucket_name}-after"
  input_list = module.bucket1a.list_of_strings
  tags = {
  "Name234" = "my-s3bucket-bucket2"
  "dependentOn" = "prefix-${module.bucket1a.bucket_arn}-suffix"
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
        let (modules_str, variables_str, outputs_str, _providers) =
            generate_full_terraform_module(&claim_modules).unwrap();
        let generated_terraform_module =
            format!("{}\n{}\n{}", modules_str, variables_str, outputs_str);

        // Two versions exist (5.81.0 and 5.95.0), ensure the latest is used
        let expected_terraform_module = r#"
terraform {
  required_providers {    
      aws = {
        source = "hashicorp/aws"
        version = "5.95.0"
      }
  }
}

module "bucket1a" {
  source = "./s3bucket-0.0.21"

  bucket_name = var.bucket1a__bucket_name
  input_list = var.bucket1a__input_list
  tags = var.bucket1a__tags
}

module "bucket2" {
  source = "./s3bucket-0.0.22"

  bucket_name = "${var.bucket1a__bucket_name}-after"
  input_list = module.bucket1a.list_of_strings
  tags = {
  "Name234" = "my-s3bucket-bucket2"
  "dependentOn" = "prefix-${module.bucket1a.bucket_arn}-suffix"
}
}

variable "bucket1a__bucket_name" {
  type = string
  description = "Name of the S3 bucket"
  nullable = false
  sensitive = false
}

variable "bucket1a__input_list" {
  type = list(string)
  default = null
  description = "Some arbitrary input list"
  nullable = true
  sensitive = false
}

variable "bucket1a__tags" {
  type = map(string)
  default = {
    "AnotherTag" = "something"
    "Test" = "hej"
  }
  description = "Tags to apply to the S3 bucket"
  nullable = true
  sensitive = false
}

output "bucket1a__bucket_arn" {
  value = module.bucket1a.bucket_arn
}

output "bucket1a__list_of_strings" {
  value = module.bucket1a.list_of_strings
}

output "bucket2__bucket_arn" {
  value = module.bucket2.bucket_arn
}

output "bucket2__list_of_strings" {
  value = module.bucket2.list_of_strings
}"#;

        assert_eq!(generated_terraform_module, expected_terraform_module);
    }

    #[test]
    fn test_validate_claim_modules_valid() {
        let yaml_manifest_bucket2 = r#"
        apiVersion: infraweave.io/v1
        kind: S3Bucket
        metadata:
            name: bucket2
        spec:
            region: N/A
            moduleVersion: 0.0.21
            variables:
                bucketName: "some-name"
                tags:
                    Name234: my-s3bucket-bucket2
                    Environment: "dev"
        "#;
        let deployment_manifest_bucket2: DeploymentManifest =
            serde_yaml::from_str(yaml_manifest_bucket2).unwrap();

        let claim_modules = [(
            deployment_manifest_bucket2,
            ModuleResp {
                s3_key: "s3bucket/s3bucket-0.0.21.zip".to_string(),
                track: "dev".to_string(),
                track_version: "dev#000.000.021".to_string(),
                version: "0.0.21".to_string(),
                timestamp: "2024-10-10T22:23:14.368+02:00".to_string(),
                module_name: "S3Bucket".to_string(),
                module_type: "module".to_string(),
                module: "s3bucket".to_string(),
                description: "Some description...".to_string(),
                reference: "".to_string(),
                manifest: ModuleManifest {
                    metadata: Metadata {
                        name: "metadata".to_string(),
                    },
                    api_version: "infraweave.io/v1".to_string(),
                    kind: "Module".to_string(),
                    spec: ModuleSpec {
                        module_name: "S3Bucket".to_string(),
                        version: Some("0.0.21".to_string()),
                        description: "Some description...".to_string(),
                        reference: "".to_string(),
                        examples: None,
                        cpu: None,
                        memory: None,
                    },
                },
                tf_outputs: vec![],
                tf_required_providers: vec![],
                tf_lock_providers: vec![],
                tf_variables: vec![
                    TfVariable {
                        name: "bucket_name".to_string(),
                        default: serde_json::Value::Null,
                        description: "Name of the S3 bucket".to_string(),
                        _type: Value::String("string".to_string()),
                        nullable: false,
                        sensitive: false,
                    },
                    TfVariable {
                        _type: Value::String("map(string)".to_string()),
                        name: "tags".to_string(),
                        description: "Tags to apply to the S3 bucket".to_string(),
                        default: serde_json::from_value(
                            serde_json::json!({"Test": "hej", "AnotherTag": "something"}),
                        )
                        .unwrap(),
                        nullable: true,
                        sensitive: false,
                    },
                ],
                stack_data: None,
                version_diff: None,
                cpu: get_default_cpu(),
                memory: get_default_memory(),
            },
        )];

        let result = validate_claim_modules(&claim_modules);
        assert_eq!(result.is_ok(), true);
    }

    #[test]
    fn test_validate_stack_name_valid() {
        let yaml_manifest = r#"
        apiVersion: infraweave.io/v1
        kind: Stack
        metadata:
            name: webpagerunner
        spec:
            stackName: WebpageRunner
            version: 0.2.1
            reference: https://github.com/your-org/webpage-runner
            description: "Webpage runner description here..."
        "#;
        let stack_manifest: StackManifest = serde_yaml::from_str(yaml_manifest).unwrap();

        let result = validate_stack_name(&stack_manifest);
        assert_eq!(result.is_ok(), true);
    }

    #[test]
    fn test_validate_stack_name_invalid() {
        let yaml_manifest = r#"
        apiVersion: infraweave.io/v1
        kind: Stack
        metadata:
            name: webpage-runner
        spec:
            stackName: WebpageRunner
            version: 0.2.1
            reference: https://github.com/your-org/webpage-runner
            description: "Webpage runner description here..."
        "#;
        let stack_manifest: StackManifest = serde_yaml::from_str(yaml_manifest).unwrap();

        let result = validate_stack_name(&stack_manifest);
        assert_eq!(result.is_err(), true);
    }

    #[test]
    fn test_validate_stack_name_invalid_must_be_lowercase_identical() {
        let yaml_manifest = r#"
        apiVersion: infraweave.io/v1
        kind: Stack
        metadata:
            name: runner
        spec:
            stackName: WebpageRunner
            version: 0.2.1
            reference: https://github.com/your-org/webpage-runner
            description: "Webpage runner description here..."
        "#;
        let stack_manifest: StackManifest = serde_yaml::from_str(yaml_manifest).unwrap();

        let result = validate_stack_name(&stack_manifest);
        assert_eq!(result.is_err(), true);
    }

    #[test]
    fn test_validate_stack_module_claim_valid() {
        let yaml_manifest_bucket = r#"
        apiVersion: infraweave.io/v1
        kind: S3Bucket
        metadata:
            name: s3bucket
        spec:
            region: N/A
            moduleVersion: 0.0.21
            variables:
                bucketName: "some-name"
        "#;
        let deployment_manifest_bucket: DeploymentManifest =
            serde_yaml::from_str(yaml_manifest_bucket).unwrap();

        let result = validate_stack_module_claim_name(&deployment_manifest_bucket);
        assert_eq!(result.is_ok(), true);
    }

    #[test]
    fn test_validate_stack_module_claim_name_should_be_lowercase_alphanumeric() {
        let yaml_manifest_bucket = r#"
        apiVersion: infraweave.io/v1
        kind: S3Bucket
        metadata:
            name: s3-bucket
        spec:
            region: N/A
            moduleVersion: 0.0.21
            variables:
                bucketName: "some-name"
        "#;
        let deployment_manifest_bucket: DeploymentManifest =
            serde_yaml::from_str(yaml_manifest_bucket).unwrap();

        let result = validate_stack_module_claim_name(&deployment_manifest_bucket);
        assert_eq!(result.is_err(), true);
    }

    #[test]
    fn test_validate_stack_module_claim_invalid_region() {
        let yaml_manifest_bucket = r#"
        apiVersion: infraweave.io/v1
        kind: S3Bucket
        metadata:
            name: s3bucket
        spec:
            region: eu-west-1
            moduleVersion: 0.0.21
            variables:
                bucketName: "some-name"
        "#;
        let deployment_manifest_bucket: DeploymentManifest =
            serde_yaml::from_str(yaml_manifest_bucket).unwrap();

        let result = validate_stack_module_claim_region_is_na(&deployment_manifest_bucket);
        assert_eq!(result.is_err(), true);
    }

    #[test]
    fn test_validate_claim_modules_namespace_should_not_be_set() {
        let yaml_manifest_bucket2 = r#"
        apiVersion: infraweave.io/v1
        kind: S3Bucket
        metadata:
            name: bucket2
            namespace: this-should-not-be-set-in-claim-for-stack
        spec:
            region: N/A
            moduleVersion: 0.0.21
            variables:
                bucketName: "some-name"
                tags:
                    Name234: my-s3bucket-bucket2
                    Environment: "dev"
        "#;
        let deployment_manifest_bucket2: DeploymentManifest =
            serde_yaml::from_str(yaml_manifest_bucket2).unwrap();

        let claim_modules = [(
            deployment_manifest_bucket2,
            ModuleResp {
                s3_key: "s3bucket/s3bucket-0.0.21.zip".to_string(),
                track: "dev".to_string(),
                track_version: "dev#000.000.021".to_string(),
                version: "0.0.21".to_string(),
                timestamp: "2024-10-10T22:23:14.368+02:00".to_string(),
                module_name: "S3Bucket".to_string(),
                module_type: "module".to_string(),
                module: "s3bucket".to_string(),
                description: "Some description...".to_string(),
                reference: "".to_string(),
                manifest: ModuleManifest {
                    metadata: Metadata {
                        name: "metadata".to_string(),
                    },
                    api_version: "infraweave.io/v1".to_string(),
                    kind: "Module".to_string(),
                    spec: ModuleSpec {
                        module_name: "S3Bucket".to_string(),
                        version: Some("0.0.21".to_string()),
                        description: "Some description...".to_string(),
                        reference: "".to_string(),
                        examples: None,
                        cpu: None,
                        memory: None,
                    },
                },
                tf_outputs: vec![],
                tf_required_providers: vec![],
                tf_lock_providers: vec![],
                tf_variables: vec![
                    TfVariable {
                        name: "bucket_name".to_string(),
                        default: serde_json::Value::Null,
                        description: "Name of the S3 bucket".to_string(),
                        _type: Value::String("string".to_string()),
                        nullable: false,
                        sensitive: false,
                    },
                    TfVariable {
                        _type: Value::String("map(string)".to_string()),
                        name: "tags".to_string(),
                        description: "Tags to apply to the S3 bucket".to_string(),
                        default: serde_json::from_value(
                            serde_json::json!({"Test": "hej", "AnotherTag": "something"}),
                        )
                        .unwrap(),
                        nullable: true,
                        sensitive: false,
                    },
                ],
                stack_data: None,
                version_diff: None,
                cpu: get_default_cpu(),
                memory: get_default_memory(),
            },
        )];

        let result = validate_claim_modules(&claim_modules);
        assert_eq!(result.is_err(), true);
    }

    #[test]
    fn test_validate_claim_modules_duplicate_claim_names() {
        let yaml_manifest_bucket1 = r#"
        apiVersion: infraweave.io/v1
        kind: S3Bucket
        metadata:
            name: bucket1
        spec:
            region: N/A
            moduleVersion: 0.0.21
            variables:
                bucketName: "some-name"
        "#;
        let deployment_manifest_bucket1: DeploymentManifest =
            serde_yaml::from_str(yaml_manifest_bucket1).unwrap();

        let yaml_manifest_bucket2 = r#"
        apiVersion: infraweave.io/v1
        kind: S3Bucket
        metadata:
            name: bucket2
        spec:
            region: N/A
            moduleVersion: 0.0.21
            variables:
                bucketName: "some-other-name"
        "#;
        let deployment_manifest_bucket2: DeploymentManifest =
            serde_yaml::from_str(yaml_manifest_bucket2).unwrap();

        let yaml_manifest_bucket3 = r#"
        apiVersion: infraweave.io/v1
        kind: S3Bucket
        metadata:
            name: bucket2
        spec:
            region: N/A
            moduleVersion: 0.0.21
            variables:
                bucketName: "some-name-duplicate"
        "#;
        let deployment_manifest_bucket3: DeploymentManifest =
            serde_yaml::from_str(yaml_manifest_bucket3).unwrap();

        let module_bucket_0_0_21 = ModuleResp {
            s3_key: "s3bucket/s3bucket-0.0.21.zip".to_string(),
            track: "dev".to_string(),
            track_version: "dev#000.000.021".to_string(),
            version: "0.0.21".to_string(),
            timestamp: "2024-10-10T22:23:14.368+02:00".to_string(),
            module_name: "S3Bucket".to_string(),
            module_type: "module".to_string(),
            module: "s3bucket".to_string(),
            description: "Some description...".to_string(),
            reference: "".to_string(),
            manifest: ModuleManifest {
                metadata: Metadata {
                    name: "metadata".to_string(),
                },
                api_version: "infraweave.io/v1".to_string(),
                kind: "Module".to_string(),
                spec: ModuleSpec {
                    module_name: "S3Bucket".to_string(),
                    version: Some("0.0.21".to_string()),
                    description: "Some description...".to_string(),
                    reference: "".to_string(),
                    examples: None,
                    cpu: None,
                    memory: None,
                },
            },
            tf_outputs: vec![],
            tf_required_providers: vec![],
            tf_lock_providers: vec![],
            tf_variables: vec![
                TfVariable {
                    name: "bucket_name".to_string(),
                    default: serde_json::Value::Null,
                    description: "Name of the S3 bucket".to_string(),
                    _type: Value::String("string".to_string()),
                    nullable: false,
                    sensitive: false,
                },
                TfVariable {
                    _type: Value::String("map(string)".to_string()),
                    name: "tags".to_string(),
                    description: "Tags to apply to the S3 bucket".to_string(),
                    default: serde_json::from_value(
                        serde_json::json!({"Test": "hej", "AnotherTag": "something"}),
                    )
                    .unwrap(),
                    nullable: true,
                    sensitive: false,
                },
            ],
            stack_data: None,
            version_diff: None,
            cpu: get_default_cpu(),
            memory: get_default_memory(),
        };

        let claim_modules = [
            (deployment_manifest_bucket1, module_bucket_0_0_21.clone()),
            (deployment_manifest_bucket2, module_bucket_0_0_21.clone()),
            (deployment_manifest_bucket3, module_bucket_0_0_21.clone()),
        ];

        let result = validate_claim_modules(&claim_modules);
        assert_eq!(result.is_ok(), false); // Should fail because the claim name bucket2 is defined twice
        let error = result.unwrap_err();
        if let ModuleError::DuplicateClaimNames(duplicate_name) = error {
            assert_eq!(duplicate_name, "bucket2");
        } else {
            panic!("Unexpected error variant: {:?}", error);
        }
    }

    #[test]
    fn test_validate_claim_modules_multiple_with_dependency_missing_output_variable() {
        let yaml_manifest_bucket1 = r#"
        apiVersion: infraweave.io/v1
        kind: S3Bucket
        metadata:
            name: bucket1a
        spec:
            region: N/A
            moduleVersion: 0.0.21
            variables:
                bucketName: "some-name"
                tags:
                    Name234: my-s3bucket-bucket1
        "#;
        let deployment_manifest_bucket1: DeploymentManifest =
            serde_yaml::from_str(yaml_manifest_bucket1).unwrap();

        let yaml_manifest_bucket2 = r#"
        apiVersion: infraweave.io/v1
        kind: S3Bucket
        metadata:
            name: bucket2
        spec:
            region: N/A
            moduleVersion: 0.0.21
            variables:
                bucketName: "some-name"
                tags:
                    Name234: my-s3bucket-bucket2
                    dependentOn: "prefix-{{ S3Bucket::bucket1a::bucketArn }}-suffix"
        "#;
        let deployment_manifest_bucket2: DeploymentManifest =
            serde_yaml::from_str(yaml_manifest_bucket2).unwrap();

        let module_bucket_0_0_21 = ModuleResp {
            s3_key: "s3bucket/s3bucket-0.0.21.zip".to_string(),
            track: "dev".to_string(),
            track_version: "dev#000.000.021".to_string(),
            version: "0.0.21".to_string(),
            timestamp: "2024-10-10T22:23:14.368+02:00".to_string(),
            module_name: "S3Bucket".to_string(),
            module_type: "module".to_string(),
            module: "s3bucket".to_string(),
            description: "Some description...".to_string(),
            reference: "".to_string(),
            manifest: ModuleManifest {
                metadata: Metadata {
                    name: "metadata".to_string(),
                },
                api_version: "infraweave.io/v1".to_string(),
                kind: "Module".to_string(),
                spec: ModuleSpec {
                    module_name: "S3Bucket".to_string(),
                    version: Some("0.0.21".to_string()),
                    description: "Some description...".to_string(),
                    reference: "".to_string(),
                    examples: None,
                    cpu: None,
                    memory: None,
                },
            },
            tf_outputs: vec![],
            tf_required_providers: vec![],
            tf_lock_providers: vec![],
            tf_variables: vec![
                TfVariable {
                    name: "bucket_name".to_string(),
                    default: serde_json::Value::Null,
                    description: "Name of the S3 bucket".to_string(),
                    _type: Value::String("string".to_string()),
                    nullable: false,
                    sensitive: false,
                },
                TfVariable {
                    _type: Value::String("map(string)".to_string()),
                    name: "tags".to_string(),
                    description: "Tags to apply to the S3 bucket".to_string(),
                    default: serde_json::from_value(
                        serde_json::json!({"Test": "hej", "AnotherTag": "something"}),
                    )
                    .unwrap(),
                    nullable: true,
                    sensitive: false,
                },
            ],
            stack_data: None,
            version_diff: None,
            cpu: get_default_cpu(),
            memory: get_default_memory(),
        };

        let claim_modules = [
            (deployment_manifest_bucket1, module_bucket_0_0_21.clone()),
            (deployment_manifest_bucket2, module_bucket_0_0_21.clone()),
        ];

        let result = validate_claim_modules(&claim_modules);
        assert_eq!(result.is_ok(), false); // Should fail because bucketArn is not defined in the module_bucket_0_0_21 output
        let error = result.unwrap_err();
        if let ModuleError::StackClaimReferenceNotFound(
            source_claim,
            kind_ref,
            claim_ref,
            variable_ref,
        ) = error
        {
            assert_eq!(source_claim, "bucket2");
            assert_eq!(kind_ref, "S3Bucket");
            assert_eq!(claim_ref, "bucket1a");
            assert_eq!(variable_ref, "bucketArn".to_string());
        } else {
            panic!("Unexpected error variant: {:?}", error);
        }
    }

    #[test]
    fn test_validate_claim_modules_multiple_with_dependency_missing_output_kind() {
        let yaml_manifest_bucket1 = r#"
        apiVersion: infraweave.io/v1
        kind: S3Bucket
        metadata:
            name: bucket1a
        spec:
            region: N/A
            moduleVersion: 0.0.21
            variables:
                bucketName: "some-name"
                tags:
                    Name234: my-s3bucket-bucket1
        "#;
        let deployment_manifest_bucket1: DeploymentManifest =
            serde_yaml::from_str(yaml_manifest_bucket1).unwrap();

        let yaml_manifest_bucket2 = r#"
        apiVersion: infraweave.io/v1
        kind: S3Bucket
        metadata:
            name: bucket2
        spec:
            region: N/A
            moduleVersion: 0.0.21
            variables:
                bucketName: "some-name"
                tags:
                    Name234: my-s3bucket-bucket2
                    dependentOn: "prefix-{{ UnknownKind::bucket1a::bucketName }}-suffix"
        "#;
        let deployment_manifest_bucket2: DeploymentManifest =
            serde_yaml::from_str(yaml_manifest_bucket2).unwrap();

        let module_bucket_0_0_21 = ModuleResp {
            s3_key: "s3bucket/s3bucket-0.0.21.zip".to_string(),
            track: "dev".to_string(),
            track_version: "dev#000.000.021".to_string(),
            version: "0.0.21".to_string(),
            timestamp: "2024-10-10T22:23:14.368+02:00".to_string(),
            module_name: "S3Bucket".to_string(),
            module_type: "module".to_string(),
            module: "s3bucket".to_string(),
            description: "Some description...".to_string(),
            reference: "".to_string(),
            manifest: ModuleManifest {
                metadata: Metadata {
                    name: "metadata".to_string(),
                },
                api_version: "infraweave.io/v1".to_string(),
                kind: "Module".to_string(),
                spec: ModuleSpec {
                    module_name: "S3Bucket".to_string(),
                    version: Some("0.0.21".to_string()),
                    description: "Some description...".to_string(),
                    reference: "".to_string(),
                    examples: None,
                    cpu: None,
                    memory: None,
                },
            },
            tf_outputs: vec![],
            tf_required_providers: vec![],
            tf_lock_providers: vec![],
            tf_variables: vec![
                TfVariable {
                    name: "bucket_name".to_string(),
                    default: serde_json::Value::Null,
                    description: "Name of the S3 bucket".to_string(),
                    _type: Value::String("string".to_string()),
                    nullable: false,
                    sensitive: false,
                },
                TfVariable {
                    _type: Value::String("map(string)".to_string()),
                    name: "tags".to_string(),
                    description: "Tags to apply to the S3 bucket".to_string(),
                    default: serde_json::from_value(
                        serde_json::json!({"Test": "hej", "AnotherTag": "something"}),
                    )
                    .unwrap(),
                    nullable: true,
                    sensitive: false,
                },
            ],
            stack_data: None,
            version_diff: None,
            cpu: get_default_cpu(),
            memory: get_default_memory(),
        };

        let claim_modules = [
            (deployment_manifest_bucket1, module_bucket_0_0_21.clone()),
            (deployment_manifest_bucket2, module_bucket_0_0_21.clone()),
        ];

        let result = validate_claim_modules(&claim_modules);
        assert_eq!(result.is_ok(), false); // Should fail because the kind UnknownKind is not a kind used in the stack
        let error = result.unwrap_err();
        if let ModuleError::StackClaimReferenceNotFound(
            source_claim,
            kind_ref,
            claim_ref,
            variable_ref,
        ) = error
        {
            assert_eq!(source_claim, "bucket2");
            assert_eq!(kind_ref, "UnknownKind");
            assert_eq!(claim_ref, "bucket1a");
            assert_eq!(variable_ref, "bucketName".to_string());
        } else {
            panic!("Unexpected error variant: {:?}", error);
        }
    }

    #[test]
    fn test_validate_claim_modules_multiple_with_dependency_correct_output() {
        let yaml_manifest_bucket1 = r#"
        apiVersion: infraweave.io/v1
        kind: S3Bucket
        metadata:
            name: bucket1a
        spec:
            region: N/A
            moduleVersion: 0.0.21
            variables:
                bucketName: "some-name"
                tags:
                    Name234: my-s3bucket-bucket1
        "#;
        let deployment_manifest_bucket1: DeploymentManifest =
            serde_yaml::from_str(yaml_manifest_bucket1).unwrap();

        let yaml_manifest_bucket2 = r#"
        apiVersion: infraweave.io/v1
        kind: S3Bucket
        metadata:
            name: bucket2
        spec:
            region: N/A
            moduleVersion: 0.0.21
            variables:
                bucketName: "some-name"
                tags:
                    Name234: my-s3bucket-bucket2
                    dependentOn: "prefix-{{ S3Bucket::bucket1a::bucketArn }}-suffix"
        "#;
        let deployment_manifest_bucket2: DeploymentManifest =
            serde_yaml::from_str(yaml_manifest_bucket2).unwrap();

        let module_bucket_0_0_21 = ModuleResp {
            s3_key: "s3bucket/s3bucket-0.0.21.zip".to_string(),
            track: "dev".to_string(),
            track_version: "dev#000.000.021".to_string(),
            version: "0.0.21".to_string(),
            timestamp: "2024-10-10T22:23:14.368+02:00".to_string(),
            module_name: "S3Bucket".to_string(),
            module_type: "module".to_string(),
            module: "s3bucket".to_string(),
            description: "Some description...".to_string(),
            reference: "".to_string(),
            manifest: ModuleManifest {
                metadata: Metadata {
                    name: "metadata".to_string(),
                },
                api_version: "infraweave.io/v1".to_string(),
                kind: "Module".to_string(),
                spec: ModuleSpec {
                    module_name: "S3Bucket".to_string(),
                    version: Some("0.0.21".to_string()),
                    description: "Some description...".to_string(),
                    reference: "".to_string(),
                    examples: None,
                    cpu: None,
                    memory: None,
                },
            },
            tf_outputs: vec![TfOutput {
                name: "bucket_arn".to_string(),
                value: "".to_string(),
                description: "ARN of the bucket".to_string(),
            }],
            tf_required_providers: vec![],
            tf_lock_providers: vec![],
            tf_variables: vec![
                TfVariable {
                    name: "bucket_name".to_string(),
                    default: serde_json::Value::Null,
                    description: "Name of the S3 bucket".to_string(),
                    _type: Value::String("string".to_string()),
                    nullable: false,
                    sensitive: false,
                },
                TfVariable {
                    _type: Value::String("map(string)".to_string()),
                    name: "tags".to_string(),
                    description: "Tags to apply to the S3 bucket".to_string(),
                    default: serde_json::from_value(
                        serde_json::json!({"Test": "hej", "AnotherTag": "something"}),
                    )
                    .unwrap(),
                    nullable: true,
                    sensitive: false,
                },
            ],
            stack_data: None,
            version_diff: None,
            cpu: get_default_cpu(),
            memory: get_default_memory(),
        };

        let claim_modules = [
            (deployment_manifest_bucket1, module_bucket_0_0_21.clone()),
            (deployment_manifest_bucket2, module_bucket_0_0_21.clone()),
        ];

        let result = validate_claim_modules(&claim_modules);
        assert_eq!(result.is_ok(), true);
    }

    #[test]
    fn test_validate_claim_modules_multiple_with_self_reference() {
        let yaml_manifest_bucket1 = r#"
        apiVersion: infraweave.io/v1
        kind: S3Bucket
        metadata:
            name: bucket1a
        spec:
            region: N/A
            moduleVersion: 0.0.21
            variables:
                bucketName: "some-name"
                tags:
                    Name234: my-s3bucket-bucket1
        "#;
        let deployment_manifest_bucket1: DeploymentManifest =
            serde_yaml::from_str(yaml_manifest_bucket1).unwrap();

        let yaml_manifest_bucket2 = r#"
        apiVersion: infraweave.io/v1
        kind: S3Bucket
        metadata:
            name: bucket2
        spec:
            region: N/A
            moduleVersion: 0.0.21
            variables:
                bucketName: "some-name"
                tags:
                    Name234: my-s3bucket-bucket2
                    dependentOn: "prefix-{{ S3Bucket::bucket2::bucketArn }}-suffix"
        "#;
        let deployment_manifest_bucket2: DeploymentManifest =
            serde_yaml::from_str(yaml_manifest_bucket2).unwrap();

        let module_bucket_0_0_21 = ModuleResp {
            s3_key: "s3bucket/s3bucket-0.0.21.zip".to_string(),
            track: "dev".to_string(),
            track_version: "dev#000.000.021".to_string(),
            version: "0.0.21".to_string(),
            timestamp: "2024-10-10T22:23:14.368+02:00".to_string(),
            module_name: "S3Bucket".to_string(),
            module_type: "module".to_string(),
            module: "s3bucket".to_string(),
            description: "Some description...".to_string(),
            reference: "".to_string(),
            manifest: ModuleManifest {
                metadata: Metadata {
                    name: "metadata".to_string(),
                },
                api_version: "infraweave.io/v1".to_string(),
                kind: "Module".to_string(),
                spec: ModuleSpec {
                    module_name: "S3Bucket".to_string(),
                    version: Some("0.0.21".to_string()),
                    description: "Some description...".to_string(),
                    reference: "".to_string(),
                    examples: None,
                    cpu: None,
                    memory: None,
                },
            },
            tf_outputs: vec![TfOutput {
                name: "bucket_arn".to_string(),
                value: "".to_string(),
                description: "ARN of the bucket".to_string(),
            }],
            tf_required_providers: vec![],
            tf_lock_providers: vec![],
            tf_variables: vec![
                TfVariable {
                    name: "bucket_name".to_string(),
                    default: serde_json::Value::Null,
                    description: "Name of the S3 bucket".to_string(),
                    _type: Value::String("string".to_string()),
                    nullable: false,
                    sensitive: false,
                },
                TfVariable {
                    _type: Value::String("map(string)".to_string()),
                    name: "tags".to_string(),
                    description: "Tags to apply to the S3 bucket".to_string(),
                    default: serde_json::from_value(
                        serde_json::json!({"Test": "hej", "AnotherTag": "something"}),
                    )
                    .unwrap(),
                    nullable: true,
                    sensitive: false,
                },
            ],
            stack_data: None,
            version_diff: None,
            cpu: get_default_cpu(),
            memory: get_default_memory(),
        };

        let claim_modules = [
            (deployment_manifest_bucket1, module_bucket_0_0_21.clone()),
            (deployment_manifest_bucket2, module_bucket_0_0_21.clone()),
        ];

        let result = validate_claim_modules(&claim_modules);
        assert_eq!(result.is_ok(), false); // Should fail because of self referencing dependency in bucket2
        let error = result.unwrap_err();
        if let ModuleError::SelfReferencingClaim(kind, claim, ref_field) = error {
            assert_eq!(kind, "S3Bucket");
            assert_eq!(claim, "bucket2");
            assert_eq!(ref_field, "bucketArn".to_string());
        } else {
            panic!("Unexpected error variant: {:?}", error);
        }
    }

    #[test]
    fn test_validate_claim_modules_multiple_with_circular_dependency() {
        let yaml_manifest_bucket1 = r#"
        apiVersion: infraweave.io/v1
        kind: S3Bucket
        metadata:
            name: bucket1
        spec:
            region: N/A
            moduleVersion: 0.0.21
            variables:
                bucketName: "some-bucket-name-1"
                tags:
                    Name234: my-s3bucket-bucket1
                    dependentOn: "prefix-{{ S3Bucket::bucket3::bucketArn }}-suffix"
        "#;
        let deployment_manifest_bucket1: DeploymentManifest =
            serde_yaml::from_str(yaml_manifest_bucket1).unwrap();

        let yaml_manifest_bucket2 = r#"
        apiVersion: infraweave.io/v1
        kind: S3Bucket
        metadata:
            name: bucket2
        spec:
            region: N/A
            moduleVersion: 0.0.21
            variables:
                bucketName: "some-bucket-name-2"
                tags:
                    Name234: my-s3bucket-bucket2
                    dependentOn: "prefix-{{ S3Bucket::bucket1::bucketArn }}-suffix"
        "#;
        let deployment_manifest_bucket2: DeploymentManifest =
            serde_yaml::from_str(yaml_manifest_bucket2).unwrap();

        let yaml_manifest_bucket3 = r#"
            apiVersion: infraweave.io/v1
            kind: S3Bucket
            metadata:
                name: bucket3
            spec:
                region: N/A
                moduleVersion: 0.0.21
                variables:
                    bucketName: "some-bucket-name-3"
                    tags:
                        Name234: my-s3bucket-bucket3
                        dependentOn: "prefix-{{ S3Bucket::bucket2::bucketArn }}-suffix"
            "#;
        let deployment_manifest_bucket3: DeploymentManifest =
            serde_yaml::from_str(yaml_manifest_bucket3).unwrap();

        let yaml_manifest_bucket4 = r#"
        apiVersion: infraweave.io/v1
        kind: S3Bucket
        metadata:
            name: bucket4
        spec:
            region: N/A
            moduleVersion: 0.0.21
            variables:
                bucketName: "some-bucket-name-4"
                tags:
                    Name234: my-s3bucket-bucket4
                    dependentOn: "prefix-{{ S3Bucket::bucket2::bucketArn }}-suffix"
        "#;
        let deployment_manifest_bucket4: DeploymentManifest =
            serde_yaml::from_str(yaml_manifest_bucket4).unwrap();

        let module_bucket_0_0_21 = ModuleResp {
            s3_key: "s3bucket/s3bucket-0.0.21.zip".to_string(),
            track: "dev".to_string(),
            track_version: "dev#000.000.021".to_string(),
            version: "0.0.21".to_string(),
            timestamp: "2024-10-10T22:23:14.368+02:00".to_string(),
            module_name: "S3Bucket".to_string(),
            module_type: "module".to_string(),
            module: "s3bucket".to_string(),
            description: "Some description...".to_string(),
            reference: "".to_string(),
            manifest: ModuleManifest {
                metadata: Metadata {
                    name: "metadata".to_string(),
                },
                api_version: "infraweave.io/v1".to_string(),
                kind: "Module".to_string(),
                spec: ModuleSpec {
                    module_name: "S3Bucket".to_string(),
                    version: Some("0.0.21".to_string()),
                    description: "Some description...".to_string(),
                    reference: "".to_string(),
                    examples: None,
                    cpu: None,
                    memory: None,
                },
            },
            tf_outputs: vec![TfOutput {
                name: "bucket_arn".to_string(),
                value: "".to_string(),
                description: "ARN of the bucket".to_string(),
            }],
            tf_required_providers: vec![],
            tf_lock_providers: vec![],
            tf_variables: vec![
                TfVariable {
                    name: "bucket_name".to_string(),
                    default: serde_json::Value::Null,
                    description: "Name of the S3 bucket".to_string(),
                    _type: Value::String("string".to_string()),
                    nullable: false,
                    sensitive: false,
                },
                TfVariable {
                    _type: Value::String("map(string)".to_string()),
                    name: "tags".to_string(),
                    description: "Tags to apply to the S3 bucket".to_string(),
                    default: serde_json::from_value(
                        serde_json::json!({"Test": "hej", "AnotherTag": "something"}),
                    )
                    .unwrap(),
                    nullable: true,
                    sensitive: false,
                },
            ],
            stack_data: None,
            version_diff: None,
            cpu: get_default_cpu(),
            memory: get_default_memory(),
        };

        let claim_modules = [
            (deployment_manifest_bucket1, module_bucket_0_0_21.clone()),
            (deployment_manifest_bucket2, module_bucket_0_0_21.clone()),
            (deployment_manifest_bucket3, module_bucket_0_0_21.clone()),
            (deployment_manifest_bucket4, module_bucket_0_0_21.clone()),
        ];

        let result = validate_claim_modules(&claim_modules);
        assert_eq!(result.is_ok(), false); // Should fail because of circular dependency
        let error = result.unwrap_err();
        if let ModuleError::CircularDependency(circular_dependencies) = error {
            assert_eq!(circular_dependencies.len(), 3);
            assert_eq!(circular_dependencies.contains(&"bucket1".to_string()), true);
            assert_eq!(circular_dependencies.contains(&"bucket2".to_string()), true);
            assert_eq!(circular_dependencies.contains(&"bucket3".to_string()), true);
        } else {
            panic!("Unexpected error variant: {:?}", error);
        }
    }

    #[test]
    fn test_validate_claim_modules_ec2_vpc_dependency_correct_output() {
        // VPC deployment manifest YAML
        let yaml_manifest_vpc = r#"
    apiVersion: infraweave.io/v1
    kind: VPC
    metadata:
      name: vpc1
    spec:
      region: N/A
      moduleVersion: 0.0.1
      variables:
        cidr: "10.0.0.0/16"
    "#;
        let deployment_manifest_vpc: DeploymentManifest =
            serde_yaml::from_str(yaml_manifest_vpc).unwrap();

        // EC2 deployment manifest YAML
        let yaml_manifest_ec2 = r#"
    apiVersion: infraweave.io/v1
    kind: EC2
    metadata:
      name: ec2instance
    spec:
      region: N/A
      moduleVersion: 0.0.1
      variables:
        instanceType: "t2.micro"
        vpcId: "{{ VPC::vpc1::vpcId }}"
    "#;
        let deployment_manifest_ec2: DeploymentManifest =
            serde_yaml::from_str(yaml_manifest_ec2).unwrap();

        // ModuleResp for the VPC.
        // Note: It must include an output named "vpc_id" (i.e. vpcId becomes vpc_id)
        let module_vpc = ModuleResp {
            s3_key: "vpc/vpc-0.0.1.zip".to_string(),
            track: "prod".to_string(),
            track_version: "prod#000.000.001".to_string(),
            version: "0.0.1".to_string(),
            timestamp: "2024-10-10T22:23:14.368+02:00".to_string(),
            module_name: "VPC".to_string(),
            module_type: "module".to_string(),
            module: "vpc".to_string(),
            description: "VPC Module".to_string(),
            reference: "".to_string(),
            manifest: ModuleManifest {
                metadata: Metadata {
                    name: "vpc-metadata".to_string(),
                },
                api_version: "infraweave.io/v1".to_string(),
                kind: "Module".to_string(),
                spec: ModuleSpec {
                    module_name: "VPC".to_string(),
                    version: Some("0.0.1".to_string()),
                    description: "VPC module description".to_string(),
                    reference: "".to_string(),
                    examples: None,
                    cpu: None,
                    memory: None,
                },
            },
            tf_outputs: vec![TfOutput {
                name: "vpc_id".to_string(),
                value: "".to_string(),
                description: "VPC Identifier".to_string(),
            }],
            tf_required_providers: vec![],
            tf_lock_providers: vec![],
            tf_variables: vec![TfVariable {
                name: "cidr".to_string(),
                default: serde_json::json!("10.0.0.0/16"),
                description: "CIDR block".to_string(),
                _type: Value::String("string".to_string()),
                nullable: false,
                sensitive: false,
            }],
            stack_data: None,
            version_diff: None,
            cpu: get_default_cpu(),
            memory: get_default_memory(),
        };

        // ModuleResp for the EC2 instance.
        let module_ec2 = ModuleResp {
            s3_key: "ec2/ec2-0.0.1.zip".to_string(),
            track: "prod".to_string(),
            track_version: "prod#000.000.001".to_string(),
            version: "0.0.1".to_string(),
            timestamp: "2024-10-10T22:23:14.368+02:00".to_string(),
            module_name: "EC2".to_string(),
            module_type: "module".to_string(),
            module: "ec2".to_string(),
            description: "EC2 Module".to_string(),
            reference: "".to_string(),
            manifest: ModuleManifest {
                metadata: Metadata {
                    name: "ec2-metadata".to_string(),
                },
                api_version: "infraweave.io/v1".to_string(),
                kind: "Module".to_string(),
                spec: ModuleSpec {
                    module_name: "EC2".to_string(),
                    version: Some("0.0.1".to_string()),
                    description: "EC2 module description".to_string(),
                    reference: "".to_string(),
                    examples: None,
                    cpu: None,
                    memory: None,
                },
            },
            tf_outputs: vec![],
            tf_required_providers: vec![],
            tf_lock_providers: vec![],
            tf_variables: vec![
                TfVariable {
                    name: "instance_type".to_string(),
                    default: serde_json::json!("t2.micro"),
                    description: "EC2 instance type".to_string(),
                    _type: Value::String("string".to_string()),
                    nullable: false,
                    sensitive: false,
                },
                TfVariable {
                    name: "vpc_id".to_string(),
                    default: serde_json::Value::Null,
                    description: "VPC ID for the EC2 instance".to_string(),
                    _type: Value::String("string".to_string()),
                    nullable: false,
                    sensitive: false,
                },
            ],
            stack_data: None,
            version_diff: None,
            cpu: get_default_cpu(),
            memory: get_default_memory(),
        };

        let claim_modules = [
            (deployment_manifest_vpc, module_vpc),
            (deployment_manifest_ec2, module_ec2),
        ];

        let result = validate_claim_modules(&claim_modules);
        // Since the dependency in EC2 ("{{ VPC::vpc1::vpcId }}") is satisfied by VPC's output "vpc_id",
        // the validation should pass.
        assert_eq!(result.is_ok(), true);
    }

    #[test]
    fn test_validate_claim_modules_nonexisting_variable() {
        let yaml_manifest_bucket2 = r#"
        apiVersion: infraweave.io/v1
        kind: S3Bucket
        metadata:
            name: bucket2
        spec:
            region: N/A
            moduleVersion: 0.0.21
            variables:
                bucketName: "some-name"
                someVariableThatDoesNotExist: "some-value"
        "#;
        let deployment_manifest_bucket2: DeploymentManifest =
            serde_yaml::from_str(yaml_manifest_bucket2).unwrap();

        let claim_modules = [(
            deployment_manifest_bucket2,
            ModuleResp {
                s3_key: "s3bucket/s3bucket-0.0.21.zip".to_string(),
                track: "dev".to_string(),
                track_version: "dev#000.000.021".to_string(),
                version: "0.0.21".to_string(),
                timestamp: "2024-10-10T22:23:14.368+02:00".to_string(),
                module_name: "S3Bucket".to_string(),
                module_type: "module".to_string(),
                module: "s3bucket".to_string(),
                description: "Some description...".to_string(),
                reference: "".to_string(),
                manifest: ModuleManifest {
                    metadata: Metadata {
                        name: "metadata".to_string(),
                    },
                    api_version: "infraweave.io/v1".to_string(),
                    kind: "Module".to_string(),
                    spec: ModuleSpec {
                        module_name: "S3Bucket".to_string(),
                        version: Some("0.0.21".to_string()),
                        description: "Some description...".to_string(),
                        reference: "".to_string(),
                        examples: None,
                        cpu: None,
                        memory: None,
                    },
                },
                tf_outputs: vec![],
                tf_required_providers: vec![],
                tf_lock_providers: vec![],
                tf_variables: vec![TfVariable {
                    name: "bucket_name".to_string(),
                    default: serde_json::Value::Null,
                    description: "Name of the S3 bucket".to_string(),
                    _type: Value::String("string".to_string()),
                    nullable: false,
                    sensitive: false,
                }],
                stack_data: None,
                version_diff: None,
                cpu: get_default_cpu(),
                memory: get_default_memory(),
            },
        )];

        let result = validate_claim_modules(&claim_modules);
        assert_eq!(result.is_err(), true);
    }

    #[test]
    fn test_validate_claim_modules_nonexisting_variable_wrongcase() {
        let yaml_manifest_bucket2 = r#"
        apiVersion: infraweave.io/v1
        kind: S3Bucket
        metadata:
            name: bucket2
        spec:
            region: N/A
            moduleVersion: 0.0.21
            variables:
                bucket_name: "some-name"
        "#;
        let deployment_manifest_bucket2: DeploymentManifest =
            serde_yaml::from_str(yaml_manifest_bucket2).unwrap();

        let claim_modules = [(
            deployment_manifest_bucket2,
            ModuleResp {
                s3_key: "s3bucket/s3bucket-0.0.21.zip".to_string(),
                track: "dev".to_string(),
                track_version: "dev#000.000.021".to_string(),
                version: "0.0.21".to_string(),
                timestamp: "2024-10-10T22:23:14.368+02:00".to_string(),
                module_name: "S3Bucket".to_string(),
                module_type: "module".to_string(),
                module: "s3bucket".to_string(),
                description: "Some description...".to_string(),
                reference: "".to_string(),
                manifest: ModuleManifest {
                    metadata: Metadata {
                        name: "metadata".to_string(),
                    },
                    api_version: "infraweave.io/v1".to_string(),
                    kind: "Module".to_string(),
                    spec: ModuleSpec {
                        module_name: "S3Bucket".to_string(),
                        version: Some("0.0.21".to_string()),
                        description: "Some description...".to_string(),
                        reference: "".to_string(),
                        examples: None,
                        cpu: None,
                        memory: None,
                    },
                },
                tf_outputs: vec![],
                tf_required_providers: vec![],
                tf_lock_providers: vec![],
                tf_variables: vec![TfVariable {
                    name: "bucket_name".to_string(),
                    default: serde_json::Value::Null,
                    description: "Name of the S3 bucket".to_string(),
                    _type: Value::String("string".to_string()),
                    nullable: false,
                    sensitive: false,
                }],
                stack_data: None,
                version_diff: None,
                cpu: get_default_cpu(),
                memory: get_default_memory(),
            },
        )];

        let result = validate_claim_modules(&claim_modules);
        assert_eq!(result.is_err(), true); // it is expecting camelCase, however it is entered as snake_case
    }

    fn get_example_claim_modules() -> Vec<(DeploymentManifest, ModuleResp)> {
        let yaml_manifest_bucket1a = r#"
    apiVersion: infraweave.io/v1
    kind: S3Bucket
    metadata:
        name: bucket1a
    spec:
        region: N/A
        moduleVersion: 0.0.21
        variables: {}
    "#;

        let yaml_manifest_bucket2 = r#"
    apiVersion: infraweave.io/v1
    kind: S3Bucket
    metadata:
        name: bucket2
    spec:
        region: N/A
        moduleVersion: 0.0.22
        variables:
            bucketName: "{{ S3Bucket::bucket1a::bucketName }}-after"
            inputList: "{{ S3Bucket::bucket1a::listOfStrings }}"
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
            (deployment_manifest_bucket1a.clone(), s3bucket_module),
            (
                deployment_manifest_bucket2.clone(),
                s3bucket_module_upgraded(),
            ),
        ];
        claim_modules
    }

    fn s3bucket_module() -> ModuleResp {
        ModuleResp {
            s3_key: "s3bucket/s3bucket-0.0.21.zip".to_string(),
            track: "dev".to_string(),
            track_version: "dev#000.000.021".to_string(),
            version: "0.0.21".to_string(),
            timestamp: "2024-10-10T22:23:14.368+02:00".to_string(),
            module_name: "S3Bucket".to_string(),
            module_type: "module".to_string(),
            module: "s3bucket".to_string(),
            description: "Some description...".to_string(),
            reference: "https://github.com/infreweave-io/modules/s3bucket".to_string(),
            manifest: ModuleManifest {
                metadata: Metadata {
                    name: "metadata".to_string(),
                },
                api_version: "infraweave.io/v1".to_string(),
                kind: "Module".to_string(),
                spec: ModuleSpec {
                    module_name: "S3Bucket".to_string(),
                    version: Some("0.0.21".to_string()),
                    description: "Some description...".to_string(),
                    reference: "https://github.com/infreweave-io/modules/s3bucket".to_string(),
                    examples: None,
                    cpu: None,
                    memory: None,
                },
            },
            tf_outputs: vec![
                TfOutput {
                    name: "bucket_arn".to_string(),
                    description: "ARN of the bucket".to_string(),
                    value: "".to_string(),
                },
                TfOutput {
                    name: "list_of_strings".to_string(),
                    description: "Made up list of strings".to_string(),
                    value: "".to_string(),
                },
                // TfOutput { name: "region".to_string(), description: "".to_string(), value: "".to_string() },
                // TfOutput { name: "sse_algorithm".to_string(), description: "".to_string(), value: "".to_string() },
            ],
            tf_required_providers: vec![TfRequiredProvider {
                name: "aws".to_string(),
                source: "hashicorp/aws".to_string(),
                version: "~> 5.0".to_string(),
            }],
            tf_lock_providers: vec![TfLockProvider {
                source: "hashicorp/aws".to_string(),
                version: "5.81.0".to_string(),
            }],
            tf_variables: vec![
                TfVariable {
                    default: serde_json::Value::Null,
                    name: "bucket_name".to_string(),
                    description: "Name of the S3 bucket".to_string(),
                    _type: Value::String("string".to_string()),
                    nullable: false,
                    sensitive: false,
                },
                // TfVariable { default: None, name: "enable_acl".to_string(), description: "Enable ACL for the S3 bucket".to_string()), _type: Value::Bool(false), nullable: Some(false), sensitive: false },
                TfVariable {
                    _type: Value::String("map(string)".to_string()),
                    name: "tags".to_string(),
                    description: "Tags to apply to the S3 bucket".to_string(),
                    default: serde_json::from_value(
                        serde_json::json!({"Test": "hej", "AnotherTag": "something"}),
                    )
                    .unwrap(),
                    nullable: true,
                    sensitive: false,
                },
                TfVariable {
                    default: serde_json::Value::Null,
                    name: "input_list".to_string(),
                    description: "Some arbitrary input list".to_string(),
                    _type: Value::String("list(string)".to_string()),
                    nullable: true,
                    sensitive: false,
                },
            ],
            stack_data: None,
            version_diff: None,
            cpu: get_default_cpu(),
            memory: get_default_memory(),
        }
    }

    fn s3bucket_module_upgraded() -> ModuleResp {
        let mut module = s3bucket_module();
        module.version = "0.0.22".to_string();
        module.s3_key = "s3bucket/s3bucket-0.0.22.zip".to_string();
        module.manifest.spec.version = Some("0.0.22".to_string());
        module.tf_required_providers = vec![TfRequiredProvider {
            name: "aws".to_string(),
            source: "hashicorp/aws".to_string(),
            version: "~> 5.0".to_string(),
        }];
        module.tf_lock_providers = vec![TfLockProvider {
            source: "hashicorp/aws".to_string(),
            version: "5.95.0".to_string(),
        }];
        module
    }
}
