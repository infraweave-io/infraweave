use anyhow::{anyhow, Result};
use base64::engine::general_purpose::STANDARD as base64;
use base64::Engine;
use env_defs::{
    get_module_identifier, CloudProvider, DeploymentManifest, DeploymentMetadata, DeploymentSpec,
    ModuleManifest, ModuleResp, ModuleVersionDiff, OciArtifactSet, ProviderResp, TfLockProvider,
    TfOutput, TfVariable,
};
use env_utils::{
    convert_module_example_variables_to_camel_case, copy_dir_recursive,
    generate_module_example_deployment, get_providers_from_lockfile,
    get_terraform_lockfile, get_tf_required_providers_from_tf_files, get_timestamp,
    get_variables_from_tf_files, merge_json_dicts, read_tf_from_zip, run_terraform_provider_lock,
    semver_parse, tempdir, validate_module_schema, validate_tf_backend_not_set,
    validate_tf_extra_environment_variables, verify_output_name_roundtrip,
    verify_variable_name_roundtrip, zero_pad_semver
};
use futures::stream::{self, StreamExt};

use hcl::expr::Variable;
use hcl::{Block, Body};
use log::{debug, info, warn};
use regex::Regex;
use std::collections::HashMap;
use std::{cmp::Ordering, path::Path};

use crate::logic::api_provider::upload_provider_cache;
use crate::logic::tf_input_resolver::TfInputResolver;
use crate::logic::tf_provider_mgmt::TfProviderMgmt;
use crate::logic::tf_root_module::{module_block, providers, variables};
use crate::{
    errors::ModuleError,
    interface::GenericCloudHandler,
    logic::{
        api_infra::{get_default_cpu, get_default_memory},
        utils::{ensure_track_matches_version, ModuleType},
    },
};

pub async fn publish_module(
    handler: &GenericCloudHandler,
    manifest_path: &str,
    track: &str,
    version_arg: Option<&str>,
    oci_artifact_set: Option<OciArtifactSet>,
) -> anyhow::Result<(), ModuleError> {
    let module_yaml_path = Path::new(manifest_path).join("module.yaml");
    let manifest =
        std::fs::read_to_string(&module_yaml_path).expect("Failed to read module manifest file");

    let mut module_yaml =
        serde_yaml::from_str::<ModuleManifest>(&manifest).expect("Failed to parse module manifest");

    validate_module_name(&module_yaml)?;
    validate_module_kind(&module_yaml)?;

    if version_arg.is_some() {
        // In case a version argument is provided
        if module_yaml.spec.version.is_some() {
            panic!("Version is not allowed when version is already set in module.yaml");
        }
        info!("Using version: {}", version_arg.as_ref().unwrap());
        module_yaml.spec.version = Some(version_arg.unwrap().to_string());
    }

    // let temp_dir = unzip_to_tempdir(zip_file).unwrap(); // TODO: no need to save to disk as intermeditary step
    let temp_dir = tempdir().map_err(|e| anyhow!(e))?;
    let temp_dir = temp_dir.path();
    let module_dir = temp_dir.join(format!(
        "{}-{}",
        module_yaml.spec.module_name.clone(),
        module_yaml
            .spec
            .version
            .clone()
            .expect("Module is missing version")
    ));

    // copy manifest_path to temp_dir
    copy_dir_recursive(Path::new(manifest_path), module_dir.as_path())
        .map_err(|e| anyhow!(e))
        .expect("Unable to copy src to build dir");
    info!(
        "Copied module at {} to {}",
        manifest_path,
        module_dir.to_str().unwrap()
    );

    // get providers metadata
    let tf_providers: Vec<ProviderResp> = get_providers_for_module(handler, &module_yaml).await?;
    if tf_providers.is_empty() {
        // TODO: May exist valid reasons to have provider-less modules, such as naming-convention modules. But not valid now.
        return Err(ModuleError::NoProvidersDefined(
            module_yaml.metadata.name.clone(),
        ));
    }

    validate_providers(&tf_providers);

    let mut tf_provider_mgmt = TfProviderMgmt::new();

    for provider in tf_providers.clone() {
        let provider_zip: Vec<u8> = download_to_vec_from_modules(handler, &provider.s3_key).await;
        let tf_content_provider = read_tf_from_zip(&provider_zip).unwrap();

        hcl::parse(&tf_content_provider)
            .expect(&format!(
                "Unable to read terraform from provider {}",
                provider.name
            ))
            .blocks()
            .for_each(|new_block| tf_provider_mgmt.add_block(new_block));
    }

    let module_tf_content = env_utils::read_tf_directory(&module_dir).unwrap();

    let mut module_inputs: Vec<Block> = Vec::new();
    hcl::parse(&module_tf_content)
        .unwrap()
        .blocks()
        .filter(|b| b.identifier() == "output" || b.identifier() == "variable")
        .for_each(|new_block| {
            if new_block.identifier() == "output" {
                let module_output_reference =
                    hcl::expr::Traversal::builder(Variable::new("module").unwrap())
                        .attr(module_yaml.metadata.name.clone())
                        .attr(new_block.labels().first().unwrap().as_str().to_string())
                        .build();
                let mut remapped_block = hcl::edit::structure::Block::from(new_block.clone());
                remapped_block.body.remove_attribute("depends_on"); //depends on can't be copied to root module, won't have access to internal resources
                let mut attr_value = remapped_block.body.get_attribute_mut("value").unwrap();
                *attr_value.value_mut() = hcl::edit::expr::Expression::from(
                    hcl::edit::expr::Traversal::from(module_output_reference),
                );
                let remapped_block = Block::from(remapped_block);
                tf_provider_mgmt.add_block(&remapped_block);
            } else if new_block.identifier() == "variable" {
                module_inputs.push(new_block.clone());
                tf_provider_mgmt.add_block(new_block)
            } else {
                panic!("Unexpected block {}", new_block.identifier())
            }
        });

    //TODO: Split into multiple files, instead of a single file
    let tf_root_providers = hcl::format::to_string(&tf_provider_mgmt.build()).unwrap();

    let deployment = DeploymentManifest {
        api_version: "infraweave.io/v1".to_string(),
        metadata: DeploymentMetadata {
            name: module_yaml.metadata.name.clone(),
            namespace: None,
            annotations: None,
            labels: None,
        },
        kind: module_yaml.spec.module_name.clone(),
        spec: DeploymentSpec {
            module_version: module_yaml.spec.version.clone(),
            stack_version: None,
            region: "N/A".to_string(),
            reference: Some(module_yaml.spec.reference.clone()),
            variables: serde_yaml::Mapping::with_capacity(0),
            dependencies: None,
            drift_detection: None,
        },
    };
    let module_call_builder = Body::builder()
        .add_block(module_block(
            &deployment,
            &variables(
                &module_inputs
                    .iter()
                    .map(|block| {
                        let name = block.labels().first().unwrap().as_str().to_string();
                        return (name.clone(), name.clone());
                    })
                    .collect(),
                &deployment,
                &TfInputResolver::new(Vec::new(), Vec::new()),
            ),
            &providers(&tf_providers),
            &Vec::with_capacity(0),
        ))
        .build();
    let tf_root_main = hcl::format::to_string(&module_call_builder).unwrap();

    info!("Root module setup:\n{}", &tf_root_providers);
    std::fs::write(temp_dir.join("providers.tf"), tf_root_providers)
        .expect("Unable to write root providers.tf");

    info!("Root module call:\n{}", &tf_root_main);
    std::fs::write(temp_dir.join("main.tf"), tf_root_main)
        .expect("Unable to write root providers.tf");

    let tf_lock_file_content = run_terraform_provider_lock(&temp_dir).await.unwrap(); // runs docker

    std::fs::write(temp_dir.join(".terraform.lock.hcl"), tf_lock_file_content)
        .expect("Unable to write lock-file to module");

    let zip_file = match env_utils::get_zip_file(Path::new(temp_dir), &module_yaml_path).await {
        Ok(zip_file) => zip_file,
        Err(error) => {
            return Err(ModuleError::ZipError(error.to_string()));
        }
    };

    // TODO: improve the ability to switch between HCL and TFVariable, such as add from_block(), to_block().
    publish_module_from_zip(
        handler,
        module_yaml,
        track,
        &zip_file,
        oci_artifact_set,
        Some(
            get_variables_from_tf_files(
                &hcl::format::to_string(&Body::builder().add_blocks(module_inputs).build())
                    .unwrap(),
            )
            .unwrap(),
        ),
    )
    .await
}

fn validate_providers(tf_providers: &Vec<ProviderResp>) {
    let mut provider_map: HashMap<String, Vec<&ProviderResp>> = HashMap::new();
    tf_providers.iter().for_each(|p| {
        let key = p.manifest.spec.configuration_name();
        if !provider_map.contains_key(&key) {
            provider_map.insert(key, Vec::new());
        }
        let provider_vec = provider_map
            .get_mut(&p.manifest.spec.configuration_name())
            .unwrap();
        provider_vec.push(p);
    });

    for (configuation_name, provider_vec) in provider_map.iter() {
        if provider_vec.len() > 1 {
            panic!(
                "configuration name \"{}\" occurs in multiple providers [{}], this is not allowed, update providers in module.yaml", 
                configuation_name,
                provider_vec.iter().map(|p|p.name.clone()).collect::<Vec<_>>().join(", "),
            );
        }
    }
}

pub async fn publish_module_from_zip(
    handler: &GenericCloudHandler,
    mut module_yaml: ModuleManifest,
    track: &str,
    zip_file: &[u8],
    oci_artifact_set: Option<OciArtifactSet>,
    module_variables: Option<Vec<TfVariable>>,
) -> Result<(), ModuleError> {
    // Encode the zip file content to Base64
    let zip_base64 = base64.encode(&zip_file);

    let tf_content = read_tf_from_zip(&zip_file).unwrap(); // Get all .tf-files concatenated into a single string

    let manifest =
        serde_yaml::to_string(&module_yaml).expect("Failed to serialize module manifest to YAML");

    match validate_tf_backend_not_set(&tf_content) {
        std::result::Result::Ok(_) => (),
        Err(error) => {
            println!("{}", error);
            std::process::exit(1);
        }
    }

    let tf_providers: Vec<ProviderResp> = get_providers_for_module(handler, &module_yaml).await?;
    if tf_providers.is_empty() {
        return Err(ModuleError::NoProvidersDefined(
            module_yaml.metadata.name.clone(),
        ));
    }

    let tf_provider_variables = tf_providers
        .iter()
        .flat_map(|provider| provider.tf_variables.clone())
        .collect::<Vec<TfVariable>>();

    match get_terraform_lockfile(&zip_file) {
        Ok(_) => {
            println!("Lock file exists, that's greate!");
        }
        Err(error) => {
            return Err(ModuleError::TerraformNoLockfile(error));
        }
    }

    match validate_module_schema(&manifest) {
        Ok(_) => (),
        Err(error) => {
            return Err(ModuleError::InvalidModuleSchema(error.to_string()));
        }
    }

    let _tf_variables = match module_variables {
        Some(vars) => vars,
        None => get_variables_from_tf_files(&tf_content)
            .unwrap()
            .iter()
            .filter(|v| !tf_provider_variables.contains(&v))
            .cloned()
            .collect(),
    };

    let tf_variables = _tf_variables
        .iter()
        .filter(|x| !x.name.starts_with("INFRAWEAVE_"))
        .cloned()
        .collect::<Vec<TfVariable>>();
    let tf_extra_environment_variables = _tf_variables
        .iter()
        .filter(|x| x.name.starts_with("INFRAWEAVE_"))
        .map(|x| x.name.clone())
        .collect::<Vec<String>>();
    let tf_outputs = hcl::parse(&tf_content)
        .expect("Failed to parse tf_content")
        .blocks()
        .filter(|b| b.identifier() == "output")
        .map(|block| TfOutput::from_block(block))
        .collect::<Result<Vec<_>, _>>()?;
    let tf_required_providers = get_tf_required_providers_from_tf_files(&tf_content).unwrap();

    if tf_required_providers.is_empty() {
        return Err(ModuleError::NoRequiredProvidersDefined(
            module_yaml.metadata.name.clone(),
        ));
    }

    validate_tf_extra_environment_variables(&tf_extra_environment_variables, &tf_variables)?;

    // Verify that all variable names can survive roundtrip case conversion
    // (snake_case -> camelCase -> snake_case)
    verify_variable_name_roundtrip(&tf_variables).map_err(|e| {
        ModuleError::InvalidVariableNaming(format!("Module '{}': {}", module_yaml.metadata.name, e))
    })?;

    // Verify that all output names can survive roundtrip case conversion
    // (snake_case -> camelCase -> snake_case)
    verify_output_name_roundtrip(&tf_outputs).map_err(|e| {
        ModuleError::InvalidOutputNaming(format!("Module '{}': {}", module_yaml.metadata.name, e))
    })?;

    let module = module_yaml.metadata.name.clone();
    let version = match module_yaml.spec.version.clone() {
        Some(version) => version,
        None => {
            return Err(ModuleError::ModuleVersionMissing(
                module_yaml.metadata.name.clone(),
            ));
        }
    };

    let manifest_version = semver_parse(&version).map_err(|e| anyhow::anyhow!(e))?;
    ensure_track_matches_version(track, &version)?;

    if let Some(ref mut examples) = module_yaml.spec.examples {
        for example in examples.iter() {
            let example_variables = &example.variables;
            let (is_valid, error) = is_all_module_example_variables_valid(
                &[&tf_variables as &[_], &tf_provider_variables].concat(),
                example_variables,
            );
            if !is_valid {
                return Err(ModuleError::InvalidExampleVariable(error));
            }
        }

        examples.iter_mut().for_each(|example| {
            example.variables = convert_module_example_variables_to_camel_case(&example.variables);
        });
    }

    info!(
        "Publishing module: {}, version \"{}.{}.{}\", pre-release/track \"{}\", build \"{}\"",
        module,
        manifest_version.major,
        manifest_version.minor,
        manifest_version.patch,
        manifest_version.pre,
        manifest_version.build
    );

    let latest_version: Option<ModuleResp> =
        match compare_latest_version(handler, &module, &version, track, ModuleType::Module).await {
            Ok(existing_version) => existing_version, // Returns existing module if newer, otherwise it's the first module version to be published
            Err(error) => {
                // If the module version already exists and is older, exit
                return Err(ModuleError::ModuleVersionExists(version, error.to_string()));
            }
        };

    if let Ok(Some(_existing_stack)) = handler.get_latest_stack_version(&module, "").await {
        return Err(ModuleError::ValidationError(format!(
            "A stack with the name '{}' already exists. Modules and stacks cannot share the same name.",
            module
        )));
    }

    let version_diff = match latest_version {
        // TODO break out to function
        Some(previous_existing_module) => {
            let current_version_module_hcl_str = &tf_content;

            // Download the previous version of the module and get hcl content
            let previous_version_s3_key = &previous_existing_module.s3_key;
            let previous_version_module_zip =
                download_to_vec_from_modules(handler, previous_version_s3_key).await;

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
                &previous_version_module_hcl_str,
                current_version_module_hcl_str,
            );

            Some(ModuleVersionDiff {
                added: additions,
                changed: changes,
                removed: deletions,
                previous_version: previous_existing_module.version.clone(),
            })
        }
        _ => None,
    };

    let tf_lock_providers: Vec<TfLockProvider> =
        get_providers_from_lockfile(&get_terraform_lockfile(&zip_file).unwrap()).unwrap();

    let module = ModuleResp {
        track: track.to_string(),
        track_version: format!(
            "{}#{}",
            track,
            zero_pad_semver(version.as_str(), 3).map_err(|e| anyhow::anyhow!(e))?
        ),
        version: version.clone(),
        timestamp: get_timestamp(),
        module: module_yaml.metadata.name.clone(),
        module_name: module_yaml.spec.module_name.clone(),
        module_type: "module".to_string(),
        description: module_yaml.spec.description.clone(),
        reference: module_yaml.spec.reference.clone(),
        manifest: module_yaml.clone(),
        tf_variables,
        tf_outputs,
        tf_providers,
        tf_required_providers,
        tf_lock_providers,
        tf_extra_environment_variables,
        s3_key: format!(
            "{}/{}-{}.zip",
            &module_yaml.metadata.name, &module_yaml.metadata.name, &version
        ), // s3_key -> "{module}/{module}-{version}.zip"
        oci_artifact_set,
        stack_data: None,
        version_diff,
        cpu: module_yaml.spec.cpu.unwrap_or_else(get_default_cpu),
        memory: module_yaml.spec.memory.unwrap_or_else(get_default_memory),
        deprecated: false,
        deprecated_message: None,
    };

    let all_regions = handler.get_all_regions().await?;

    // Handle module publishing and provider uploads based on whether OCI is available
    match &handler.get_oci_client() {
        Some(oci_client) => {
            // When using OCI, only upload the OCI artifact
            println!("Publishing module to OCI registry...");
            oci_client.upload_module(&module, &zip_base64).await?;
            info!("Successfully completed OCI module publishing");
        }
        None => {
            // Check if TEST_MODE is enabled to determine concurrency limit
            let is_test_mode = std::env::var("TEST_MODE")
                .map(|val| val.to_lowercase() == "true" || val == "1")
                .unwrap_or(false);

            let concurrency_limit_env = std::env::var("CONCURRENCY_LIMIT")
                .unwrap_or_else(|_| "".to_string())
                .parse::<usize>()
                .unwrap_or(10);

            let effective_concurrency_limit = if is_test_mode {
                debug!("TEST_MODE enabled, limiting all upload operations to concurrency of 1");
                1
            } else {
                concurrency_limit_env
            };

            println!("Publishing module and ensuring providers in all regions with concurrency limit: {}", effective_concurrency_limit);

            // Combine all upload tasks into a single vector using boxed futures
            let mut all_upload_tasks: Vec<
                std::pin::Pin<
                    Box<dyn std::future::Future<Output = Result<(), ModuleError>> + Send>,
                >,
            > = Vec::new();

            // Add provider upload tasks
            for region in all_regions.iter() {
                for provider in module.tf_lock_providers.iter() {
                    let handler = handler.clone();
                    let region = region.clone();
                    let provider = provider.clone();

                    let task = Box::pin(async move {
                        let region_handler = handler.copy_with_region(&region).await;
                        match upload_provider_cache(&region_handler, &provider).await {
                            Ok(_) => {
                                println!(
                                    "Ensured provider {} ({}) is cached in region {}",
                                    provider.source, provider.version, region
                                );
                                Ok(())
                            }
                            Err(error) => Err(ModuleError::UploadModuleError(format!(
                                "Failed to upload provider {} to region {}: {}",
                                provider.source, region, error
                            ))),
                        }
                    });
                    all_upload_tasks.push(task);
                }
            }

            // Add module upload tasks
            for region in all_regions.iter() {
                let handler = handler.clone();
                let region = region.clone();
                let module_ref = module.clone();
                let zip_base64_ref = zip_base64.clone();

                let task = Box::pin(async move {
                    let region_handler = handler.copy_with_region(&region).await;
                    match upload_module(&region_handler, &module_ref, &zip_base64_ref).await {
                        Ok(_) => {
                            println!(
                                "Module {} is cached in region {}",
                                module_ref.module, region
                            );
                            Ok(())
                        }
                        Err(error) => Err(ModuleError::UploadModuleError(format!(
                            "Failed to upload module {} to region {}: {}",
                            module_ref.module, region, error
                        ))),
                    }
                });
                all_upload_tasks.push(task);
            }

            let concurrency_limit =
                std::cmp::min(all_upload_tasks.len(), effective_concurrency_limit);
            info!(
                "Executing {} upload tasks with concurrency limit of {}",
                all_upload_tasks.len(),
                concurrency_limit
            );

            // Execute all tasks with the specified concurrency limit
            let results: Vec<Result<(), ModuleError>> = stream::iter(all_upload_tasks)
                .buffer_unordered(concurrency_limit)
                .collect()
                .await;

            // Check if any uploads failed
            for result in results {
                result?;
            }

            info!("Successfully completed all provider and module uploads");
        }
    }

    Ok(())
}

pub async fn get_providers_for_module(
    handler: &GenericCloudHandler,
    module: &ModuleManifest,
) -> Result<Vec<ProviderResp>, anyhow::Error> {
    let mut providers: Vec<ProviderResp> = vec![];
    for provider in module.spec.providers.iter() {
        match handler.get_latest_provider_version(&provider.name).await {
            Ok(provider_result) => match provider_result {
                Some(provider) => providers.push(provider),
                None => {
                    return Err(anyhow::anyhow!(
                        "No provider found with name: {}",
                        provider.name
                    ));
                }
            },
            Err(error) => {
                panic!(
                    "Failed to get latest provider {} version: {}",
                    provider.name, error
                );
            }
        }
    }
    Ok(providers)
}

fn validate_module_name(module_manifest: &ModuleManifest) -> anyhow::Result<(), ModuleError> {
    let name = module_manifest.metadata.name.clone();
    let module_name = module_manifest.spec.module_name.clone();
    let re = Regex::new(r"^[a-z][a-z0-9]+$").unwrap();
    if !re.is_match(&name) {
        return Err(ModuleError::ValidationError(format!(
            "Module name {} must only use lowercase characters and numbers.",
            name,
        )));
    }
    if !module_name.chars().next().unwrap().is_uppercase() {
        return Err(ModuleError::ValidationError(format!(
            "The moduleName {} must start with an uppercase character.",
            module_name
        )));
    }
    if !module_name.chars().all(|c| c.is_alphanumeric()) {
        return Err(ModuleError::ValidationError(format!(
            "The moduleName {} must only contain alphanumeric characters (no hyphens, underscores, or special characters).",
            module_name
        )));
    }
    if module_name.to_lowercase() != name {
        return Err(ModuleError::ValidationError(format!(
            "The name {} must exactly match lowercase of the moduleName specified under spec {}.",
            name, module_name
        )));
    }
    Ok(())
}

fn validate_module_kind(module_manifest: &ModuleManifest) -> anyhow::Result<(), ModuleError> {
    let kind = module_manifest.kind.clone();
    if kind != "Module" {
        return Err(ModuleError::ValidationError(format!(
            "The kind field in module.yaml must be 'Module', but found '{}'.",
            kind
        )));
    }
    Ok(())
}

pub async fn upload_module(
    handler: &GenericCloudHandler,
    module: &ModuleResp,
    zip_base64: &String,
) -> anyhow::Result<(), anyhow::Error> {
    let payload = serde_json::json!({
        "event": "upload_file_base64",
        "data":
        {
            "key": &module.s3_key,
            "bucket_name": "modules",
            "base64_content": &zip_base64
        }

    });
    match handler.run_function(&payload).await {
        Ok(_) => {
            info!("Successfully uploaded module zip file to storage");
        }
        Err(error) => {
            return Err(anyhow::anyhow!("{}", error));
        }
    }

    match insert_module(handler, module).await {
        Ok(_) => {
            info!(
                "Successfully published module {} metadata in region",
                module.module
            );
        }
        Err(error) => {
            return Err(anyhow::anyhow!("{}", error));
        }
    }

    info!(
        "Publishing version {} of module {}",
        module.version, module.module
    );

    Ok(())
}

pub async fn insert_module(
    handler: &GenericCloudHandler,
    module: &ModuleResp,
) -> anyhow::Result<String> {
    let module_table_placeholder = "modules";

    let mut transaction_items = vec![];

    let id: String = format!(
        "MODULE#{}",
        get_module_identifier(&module.module, &module.track)
    );

    // -------------------------
    // Module metadata
    // -------------------------
    let mut module_payload = serde_json::to_value(serde_json::json!({
        "PK": id.clone(),
        "SK": format!("VERSION#{}", zero_pad_semver(&module.version, 3)?),
    }))
    .unwrap();

    let module_value = serde_json::to_value(module)?;
    merge_json_dicts(&mut module_payload, &module_value);

    transaction_items.push(serde_json::json!({
        "Put": {
            "TableName": module_table_placeholder,
            "Item": module_payload
        }
    }));

    // -------------------------
    // Latest module version
    // -------------------------
    // It is inserted as a MODULE (above) but LATEST-prefix is used to differentiate stack and module (to reduce maintenance)
    let latest_pk = if module.stack_data.is_some() {
        "LATEST_STACK"
    } else {
        "LATEST_MODULE"
    };
    let mut latest_module_payload = serde_json::to_value(serde_json::json!({
        "PK": latest_pk,
        "SK": id.clone(),
    }))?;

    // Use the same module metadata to the latest module version
    merge_json_dicts(&mut latest_module_payload, &module_value);

    transaction_items.push(serde_json::json!({
        "Put": {
            "TableName": module_table_placeholder,
            "Item": latest_module_payload
        }
    }));

    // -------------------------
    // Execute the Transaction
    // -------------------------

    let payload = serde_json::json!({
        "event": "transact_write",
        "items": transaction_items,
    });
    match handler.run_function(&payload).await {
        Ok(response) => Ok(response.payload.to_string()),
        Err(e) => Err(e),
    }
}

pub async fn deprecate_module(
    handler: &GenericCloudHandler,
    module: &str,
    track: &str,
    version: &str,
    message: Option<&str>,
) -> anyhow::Result<()> {
    info!(
        "Deprecating module: {}, track: {}, version: {}",
        module, track, version
    );

    // First, fetch the existing module version to ensure it exists and get all its data
    let existing_module = match handler.get_module_version(module, track, version).await? {
        Some(module) => module,
        None => {
            return Err(anyhow!(
                "Module {} version {} not found in track {}",
                module,
                version,
                track
            ));
        }
    };

    // Check if this version is already deprecated
    if existing_module.deprecated {
        return Err(anyhow!(
            "Module {} version {} is already deprecated",
            module,
            version
        ));
    }

    // Check if this is the latest version - we don't allow deprecating the latest version
    let latest_module = if existing_module.stack_data.is_some() {
        handler.get_latest_stack_version(module, track).await?
    } else {
        handler.get_latest_module_version(module, track).await?
    };

    if let Some(latest) = latest_module {
        if latest.version == version {
            return Err(anyhow!(
                "Cannot deprecate the latest version ({}) of module {} in track {}.\n\
                Please publish a new version that resolves the issue before deprecating this version.",
                version,
                module,
                track
            ));
        }
    }

    let module_table_placeholder = "modules";
    let mut transaction_items = vec![];

    let id: String = format!("MODULE#{}", get_module_identifier(module, track));

    // Update the specific version record
    let mut module_payload = serde_json::to_value(serde_json::json!({
        "PK": id.clone(),
        "SK": format!("VERSION#{}", zero_pad_semver(version, 3)?),
    }))
    .unwrap();

    // Serialize the existing module with deprecated flag set to true and optional message
    let mut updated_module = existing_module.clone();
    updated_module.deprecated = true;
    updated_module.deprecated_message = message.map(|s| s.to_string());
    let module_value = serde_json::to_value(&updated_module)?;
    merge_json_dicts(&mut module_payload, &module_value);

    transaction_items.push(serde_json::json!({
        "Put": {
            "TableName": module_table_placeholder,
            "Item": module_payload
        }
    }));

    // Execute the Transaction
    let payload = serde_json::json!({
        "event": "transact_write",
        "items": transaction_items,
    });

    // Get all regions to update deprecation status across all of them
    let all_regions = handler.get_all_regions().await?;

    info!("Deprecating module in all regions...");

    // Update module in all regions
    for region in all_regions.iter() {
        let region_handler = handler.copy_with_region(region).await;

        match region_handler.run_function(&payload).await {
            Ok(_) => {
                info!(
                    "Successfully deprecated module {} version {} in track {} in region {}",
                    module, version, track, region
                );
            }
            Err(e) => {
                return Err(anyhow!(
                    "Failed to deprecate module in region {}: {}",
                    region,
                    e
                ));
            }
        }
    }

    info!(
        "Successfully deprecated module {} version {} in track {} in all regions",
        module, version, track
    );

    Ok(())
}

pub async fn compare_latest_version(
    handler: &GenericCloudHandler,
    module: &str,
    version: &str,
    track: &str,
    module_type: ModuleType,
) -> Result<Option<ModuleResp>, anyhow::Error> {
    if version.starts_with("0.0.0") {
        warn!("Skipping version check for unreleased version {}", version);
        return Ok(None); // Used for unreleased versions (for testing in pipeline)
    }

    let fetch_module: Result<Option<ModuleResp>, anyhow::Error> = match module_type {
        ModuleType::Module => handler.get_latest_module_version(module, track).await,
        ModuleType::Stack => handler.get_latest_stack_version(module, track).await,
    };

    let entity = if module_type == ModuleType::Module {
        "Module"
    } else {
        "Stack"
    };

    match fetch_module {
        Ok(fetch_module) => {
            if let Some(latest_module) = fetch_module {
                let manifest_version = env_utils::semver_parse(version)?;
                let latest_version = env_utils::semver_parse(&latest_module.version)?;

                // Since semver crate breaks the semver spec (to follow cargo-variant) by also comparing build numbers, we need to compare without build
                // https://github.com/dtolnay/semver/issues/172
                let manifest_version_no_build = env_utils::semver_parse_without_build(version)?;
                let latest_version_no_build =
                    env_utils::semver_parse_without_build(&latest_module.version)?;

                debug!("manifest_version: {:?}", manifest_version);
                debug!("latest_version: {:?}", latest_version);

                match manifest_version_no_build.cmp(&latest_version_no_build) {
                    Ordering::Equal => {
                        // Same version number, check build
                        if manifest_version.build == latest_version.build {
                            Err(anyhow::anyhow!(
                                "{} version {} already exists in track {}",
                                entity,
                                manifest_version,
                                track
                            ))
                        } else {
                            info!(
                                "Newer build version of same version {} => {}",
                                latest_version.build, manifest_version.build
                            );
                            Ok(Some(latest_module))
                        }
                    }

                    Ordering::Less => Err(anyhow::anyhow!(
                        "{} version {} is older than the latest version {} in track {}",
                        entity,
                        manifest_version,
                        latest_version,
                        track
                    )),

                    Ordering::Greater => {
                        info!(
                            "{} version {} is confirmed to be the newest version",
                            entity, manifest_version
                        );
                        Ok(Some(latest_module))
                    }
                }
            } else {
                info!(
                    "No existing {} version found in track {}, this is the first version",
                    entity, track
                );
                Ok(None)
            }
        }
        Err(e) => Err(anyhow::anyhow!("An error occurred: {:?}", e)),
    }
}

pub async fn download_to_vec_from_modules(
    handler: &GenericCloudHandler,
    s3_key: &String,
) -> Vec<u8> {
    info!("Downloading module from {}...", s3_key);

    let url = match get_modules_download_url(handler, s3_key).await {
        Ok(url) => url,
        Err(e) => {
            panic!("Error: {:?}", e);
        }
    };

    match env_utils::download_zip_to_vec(&url).await {
        Ok(content) => {
            info!("Downloaded module");
            content
        }
        Err(e) => {
            panic!("Error: {:?}", e);
        }
    }
}

pub async fn get_modules_download_url(
    handler: &GenericCloudHandler,
    key: &str,
) -> Result<String, anyhow::Error> {
    let url = match handler.generate_presigned_url(key, "modules").await {
        Ok(response) => response,
        Err(e) => {
            return Err(anyhow::anyhow!("Failed to read db: {}", e));
        }
    };
    Ok(url)
}

pub async fn precheck_module(manifest_path: &String) -> anyhow::Result<(), anyhow::Error> {
    let module_yaml_path = Path::new(manifest_path).join("module.yaml");
    let manifest =
        std::fs::read_to_string(&module_yaml_path).expect("Failed to read module manifest file");

    let module_yaml =
        serde_yaml::from_str::<ModuleManifest>(&manifest).expect("Failed to parse module manifest");

    // println!("Prechecking module: {}", module_yaml.metadata.name);
    // println!("Full module: {:#?}", module_yaml);
    // println!("Examples: {:#?}", module_yaml.spec.examples);

    let module_spec = &module_yaml.spec.clone();
    let examples = &module_spec.examples;

    if let Some(examples) = examples {
        for example in examples {
            let example_claim = generate_module_example_deployment(module_spec, example);
            let claim_str = serde_yaml::to_string(&example_claim)?;
            info!("{}", claim_str);
        }
    } else {
        info!("No examples found in module.yaml, consider adding some to guide your users");
    }

    Ok(())
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
    // Check that all variables in example_variables are valid
    for (key, value) in example_variables.iter() {
        let key_str = key.as_str().unwrap();
        // Check if variable is snake_case
        if key_str != env_utils::to_snake_case(key_str) {
            let error = format!(
                "Example variable {} is not snake_case like the terraform variable",
                key_str
            );
            return (false, error); // Example-variable is not snake_case
        }
        let tf_variable = tf_variables.iter().find(|&x| x.name == key_str);
        if tf_variable.is_none() {
            let error = format!("Example variable {} does not exist", key_str);
            return (false, error); // Example-variable does not exist
        }
        let tf_variable = tf_variable.unwrap();
        let is_nullable = tf_variable.nullable;
        if (tf_variable.default == Some(serde_json::Value::Null) || tf_variable.default.is_none())
            && !is_nullable
            && value.is_null()
        {
            let error = format!("Required variable {} is null but mandatory", key_str);
            return (false, error); // Required variable is null
        }
    }
    // Check that all required variables are present in example_variables
    for tf_variable in tf_variables.iter() {
        let is_nullable = tf_variable.nullable;
        if (tf_variable.default == Some(serde_json::Value::Null) || tf_variable.default.is_none())
            && !is_nullable
        {
            // This is a required variable
            let variable_exists = example_variables
                .contains_key(&serde_yaml::Value::String(tf_variable.name.clone()));
            if !variable_exists {
                let error = format!("Required variable {} is missing", tf_variable.name);
                return (false, error); // Required variable is missing
            }
        }
    }
    (true, "".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use env_defs::{ProviderManifest, ProviderMetaData, ProviderSpec};
    use pretty_assertions::assert_eq;

    #[test]
    fn test_is_example_variables_valid() {
        let tf_variables = vec![
            TfVariable {
                name: "bucket_name".to_string(),
                description: "The name of the bucket".to_string(),
                default: None,
                sensitive: false,
                nullable: false,
                _type: serde_json::Value::String("string".to_string()),
            },
            TfVariable {
                name: "tags".to_string(),
                description: "The tags to apply to the bucket".to_string(),
                default: None,
                sensitive: false,
                nullable: false,
                _type: serde_json::Value::String("map".to_string()),
            },
            TfVariable {
                name: "port_mapping".to_string(),
                description: "The port mapping".to_string(),
                default: None,
                sensitive: false,
                nullable: false,
                _type: serde_json::Value::String("list".to_string()),
            },
        ];
        let example_variables = serde_yaml::from_str::<serde_yaml::Value>(
            r#"
bucket_name: some-bucket-name
tags:
  oneTag: value1
  anotherTag: value2
port_mapping:
  - containerPort: 80
    hostPort: 80
"#,
        )
        .unwrap();
        let (is_valid, _error) =
            is_all_module_example_variables_valid(&tf_variables, &example_variables);
        assert_eq!(is_valid, true);
    }

    #[test]
    fn test_is_example_variables_valid_true_has_default() {
        let tf_variables = vec![
            TfVariable {
                name: "instance_name".to_string(),
                description: "Instance name".to_string(),
                default: Some(serde_json::Value::String("my-instance".to_string())),
                sensitive: false,
                nullable: false,
                _type: serde_json::Value::String("string".to_string()),
            },
            TfVariable {
                name: "bucket_name".to_string(),
                description: "Bucket name".to_string(),
                default: None,
                sensitive: false,
                nullable: false,
                _type: serde_json::Value::String("string".to_string()),
            },
        ];
        let example_variables = serde_yaml::from_str::<serde_yaml::Value>(
            r#"
bucket_name: some-bucket-name
"#,
        )
        .unwrap();
        let (is_valid, _error) =
            is_all_module_example_variables_valid(&tf_variables, &example_variables);
        assert_eq!(is_valid, true);
    }

    #[test]
    fn test_is_example_variables_valid_false_has_no_default() {
        let tf_variables = vec![
            TfVariable {
                name: "instance_name".to_string(),
                description: "Instance name".to_string(),
                default: None,
                sensitive: false,
                nullable: false,
                _type: serde_json::Value::String("string".to_string()),
            },
            TfVariable {
                name: "bucket_name".to_string(),
                description: "Bucket name".to_string(),
                default: None,
                sensitive: false,
                nullable: false,
                _type: serde_json::Value::String("string".to_string()),
            },
        ];
        let example_variables = serde_yaml::from_str::<serde_yaml::Value>(
            r#"
bucket_name: some-bucket-name
"#,
        )
        .unwrap();
        let (is_valid, _error) =
            is_all_module_example_variables_valid(&tf_variables, &example_variables);
        assert_eq!(is_valid, false);
    }

    #[test]
    fn test_is_example_variables_valid_true_has_no_default_but_nullable() {
        let tf_variables = vec![
            TfVariable {
                name: "instance_name".to_string(),
                description: "Instance name".to_string(),
                default: None,
                sensitive: false,
                nullable: true,
                _type: serde_json::Value::String("string".to_string()),
            },
            TfVariable {
                name: "bucket_name".to_string(),
                description: "Bucket name".to_string(),
                default: None,
                sensitive: false,
                nullable: false,
                _type: serde_json::Value::String("string".to_string()),
            },
        ];
        let example_variables = serde_yaml::from_str::<serde_yaml::Value>(
            r#"
bucket_name: some-bucket-name
"#,
        )
        .unwrap();
        let (is_valid, _error) =
            is_all_module_example_variables_valid(&tf_variables, &example_variables);
        assert_eq!(is_valid, true);
    }

    #[test]
    fn test_is_example_variables_valid_false_required_missing() {
        let tf_variables = vec![TfVariable {
            name: "bucket_name".to_string(),
            description: "The name of the bucket".to_string(),
            default: None,
            sensitive: false,
            nullable: false,
            _type: serde_json::Value::String("string".to_string()),
        }];
        let example_variables = serde_yaml::from_str::<serde_yaml::Value>(
            r#"
tags:
  oneTag: value1
  anotherTag: value2
port_mapping:
  - containerPort: 80
    hostPort: 80
"#,
        )
        .unwrap();
        let (is_valid, _error) =
            is_all_module_example_variables_valid(&tf_variables, &example_variables);
        assert_eq!(is_valid, false);
    }

    #[test]
    fn test_is_example_variables_snake_case_false() {
        let tf_variables = vec![TfVariable {
            name: "bucketName".to_string(),
            description: "Bucket name".to_string(),
            default: None,
            sensitive: false,
            nullable: false,
            _type: serde_json::Value::String("string".to_string()),
        }];
        let example_variables = serde_yaml::from_str::<serde_yaml::Value>(
            r#"
bucketName: some-bucket-name
"#,
        )
        .unwrap();
        let (is_valid, _error) =
            is_all_module_example_variables_valid(&tf_variables, &example_variables);
        assert_eq!(is_valid, false);
    }

    #[test]
    fn test_validate_module_name_valid() {
        let yaml_manifest = r#"
        apiVersion: infraweave.io/v1
        kind: Module
        metadata:
            name: s3bucket
        spec:
            moduleName: S3Bucket
            version: 0.2.1
            providers: []
            reference: https://github.com/your-org/s3bucket
            description: "S3Bucket description here..."
        "#;
        let module_manifest: ModuleManifest = serde_yaml::from_str(yaml_manifest).unwrap();

        let result = validate_module_name(&module_manifest);
        assert_eq!(result.is_ok(), true);
    }

    #[test]
    fn test_validate_module_name_invalid() {
        let yaml_manifest = r#"
        apiVersion: infraweave.io/v1
        kind: Module
        metadata:
            name: s3-bucket
        spec:
            moduleName: S3Bucket
            version: 0.2.1
            providers: []
            reference: https://github.com/your-org/s3bucket
            description: "module_manifest description here..."
        "#;
        let module_manifest: ModuleManifest = serde_yaml::from_str(yaml_manifest).unwrap();

        let result = validate_module_name(&module_manifest);
        assert_eq!(result.is_err(), true);
    }

    #[test]
    fn test_validate_module_name_invalid_must_be_lowercase_identical() {
        let yaml_manifest = r#"
        apiVersion: infraweave.io/v1
        kind: Module
        metadata:
            name: bucket
        spec:
            moduleName: S3Bucket
            version: 0.2.1
            providers: []
            reference: https://github.com/your-org/s3bucket
            description: "module_manifest description here..."
        "#;
        let module_manifest: ModuleManifest = serde_yaml::from_str(yaml_manifest).unwrap();

        let result = validate_module_name(&module_manifest);
        assert_eq!(result.is_err(), true);
    }

    #[test]
    fn test_validate_module_kind_valid() {
        let yaml_manifest = r#"
        apiVersion: infraweave.io/v1
        kind: Module
        metadata:
            name: s3bucket
        spec:
            moduleName: S3Bucket
            version: 0.2.1
            providers: []
            reference: https://github.com/your-org/s3bucket
            description: "S3Bucket description here..."
        "#;
        let module_manifest: ModuleManifest = serde_yaml::from_str(yaml_manifest).unwrap();

        let result = validate_module_kind(&module_manifest);
        assert_eq!(result.is_ok(), true);
    }

    #[test]
    fn test_validate_module_kind_invalid() {
        let yaml_manifest = r#"
        apiVersion: infraweave.io/v1
        kind: Manifest
        metadata:
            name: s3bucket
        spec:
            moduleName: S3Bucket
            version: 0.2.1
            providers: []
            reference: https://github.com/your-org/s3bucket
            description: "S3Bucket description here..."
        "#;
        let module_manifest: ModuleManifest = serde_yaml::from_str(yaml_manifest).unwrap();

        let result = validate_module_kind(&module_manifest);
        assert_eq!(result.is_err(), true);
    }

    #[test]
    fn test_validate_module_name_must_start_with_uppercase() {
        let yaml_manifest = r#"
        apiVersion: infraweave.io/v1
        kind: Module
        metadata:
            name: s3bucket
        spec:
            moduleName: s3Bucket
            version: 0.2.1
            providers: []
            reference: https://github.com/your-org/s3bucket
            description: "S3Bucket description here..."
        "#;
        let module_manifest: ModuleManifest = serde_yaml::from_str(yaml_manifest).unwrap();

        let result = validate_module_name(&module_manifest);
        assert_eq!(result.is_err(), true);
    }

    #[test]
    fn test_validate_module_name_must_be_alphanumeric() {
        let yaml_manifest = r#"
        apiVersion: infraweave.io/v1
        kind: Module
        metadata:
            name: s3bucket
        spec:
            moduleName: S3-Bucket
            version: 0.2.1
            providers: []
            reference: https://github.com/your-org/s3bucket
            description: "S3Bucket description here..."
        "#;
        let module_manifest: ModuleManifest = serde_yaml::from_str(yaml_manifest).unwrap();

        let result = validate_module_name(&module_manifest);
        assert_eq!(result.is_err(), true);
    }

    #[test]
    fn test_validate_module_name_must_be_alphanumeric_no_underscore() {
        let yaml_manifest = r#"
        apiVersion: infraweave.io/v1
        kind: Module
        metadata:
            name: s3_bucket
        spec:
            moduleName: S3_Bucket
            version: 0.2.1
            providers: []
            reference: https://github.com/your-org/s3bucket
            description: "S3Bucket description here..."
        "#;
        let module_manifest: ModuleManifest = serde_yaml::from_str(yaml_manifest).unwrap();

        let result = validate_module_name(&module_manifest);
        assert_eq!(result.is_err(), true);
    }

    #[test]
    fn test_validate_providers_all_ok() {
        let tf_providers = vec![
            ProviderResp {
                name: "helm".to_string(),
                version: "1.0.0".to_string(),
                timestamp: "".to_string(),
                description: "".to_string(),
                reference: "".to_string(),
                manifest: ProviderManifest {
                    metadata: ProviderMetaData {
                        name: "helm-3".to_string(),
                    },
                    api_version: "".to_string(),
                    kind: "".to_string(),
                    spec: ProviderSpec {
                        provider: "helm".to_string(),
                        alias: Some("us-east-1".to_string()),
                        version: None,
                        description: "".to_string(),
                        reference: "".to_string(),
                    },
                },
                tf_variables: Vec::with_capacity(0),
                tf_extra_environment_variables: Vec::with_capacity(0),
                s3_key: "".to_string(),
            },
            ProviderResp {
                name: "aws2".to_string(),
                version: "1.0.0".to_string(),
                timestamp: "".to_string(),
                description: "".to_string(),
                reference: "".to_string(),
                manifest: ProviderManifest {
                    metadata: ProviderMetaData {
                        name: "aws-5".to_string(),
                    },
                    api_version: "".to_string(),
                    kind: "".to_string(),
                    spec: ProviderSpec {
                        provider: "aws".to_string(),
                        alias: Some("us-east-1".to_string()),
                        version: None,
                        description: "".to_string(),
                        reference: "".to_string(),
                    },
                },
                tf_variables: Vec::with_capacity(0),
                tf_extra_environment_variables: Vec::with_capacity(0),
                s3_key: "".to_string(),
            },
        ];
        validate_providers(&tf_providers);
    }

    #[test]
    fn test_validate_ok_same_name_different_alias() {
        let tf_providers = vec![
            ProviderResp {
                name: "aws1".to_string(),
                version: "1.0.0".to_string(),
                timestamp: "".to_string(),
                description: "".to_string(),
                reference: "".to_string(),
                manifest: ProviderManifest {
                    metadata: ProviderMetaData {
                        name: "aws-5".to_string(),
                    },
                    api_version: "".to_string(),
                    kind: "".to_string(),
                    spec: ProviderSpec {
                        provider: "aws".to_string(),
                        alias: Some("us-east-1".to_string()),
                        version: None,
                        description: "".to_string(),
                        reference: "".to_string(),
                    },
                },
                tf_variables: Vec::with_capacity(0),
                tf_extra_environment_variables: Vec::with_capacity(0),
                s3_key: "".to_string(),
            },
            ProviderResp {
                name: "aws2".to_string(),
                version: "1.0.0".to_string(),
                timestamp: "".to_string(),
                description: "".to_string(),
                reference: "".to_string(),
                manifest: ProviderManifest {
                    metadata: ProviderMetaData {
                        name: "aws-5".to_string(),
                    },
                    api_version: "".to_string(),
                    kind: "".to_string(),
                    spec: ProviderSpec {
                        provider: "aws".to_string(),
                        alias: Some("us-east-2".to_string()),
                        version: None,
                        description: "".to_string(),
                        reference: "".to_string(),
                    },
                },
                tf_variables: Vec::with_capacity(0),
                tf_extra_environment_variables: Vec::with_capacity(0),
                s3_key: "".to_string(),
            },
        ];
        validate_providers(&tf_providers);
    }

    #[test]
    #[should_panic]
    fn test_validate_panic_on_duplicate_config_name() {
        let tf_providers = vec![
            ProviderResp {
                name: "aws1".to_string(),
                version: "1.0.0".to_string(),
                timestamp: "".to_string(),
                description: "".to_string(),
                reference: "".to_string(),
                manifest: ProviderManifest {
                    metadata: ProviderMetaData {
                        name: "aws-5".to_string(),
                    },
                    api_version: "".to_string(),
                    kind: "".to_string(),
                    spec: ProviderSpec {
                        provider: "aws".to_string(),
                        alias: Some("us-east-1".to_string()),
                        version: None,
                        description: "".to_string(),
                        reference: "".to_string(),
                    },
                },
                tf_variables: Vec::with_capacity(0),
                tf_extra_environment_variables: Vec::with_capacity(0),
                s3_key: "".to_string(),
            },
            ProviderResp {
                name: "aws2".to_string(),
                version: "1.0.0".to_string(),
                timestamp: "".to_string(),
                description: "".to_string(),
                reference: "".to_string(),
                manifest: ProviderManifest {
                    metadata: ProviderMetaData {
                        name: "aws-5".to_string(),
                    },
                    api_version: "".to_string(),
                    kind: "".to_string(),
                    spec: ProviderSpec {
                        provider: "aws".to_string(),
                        alias: Some("us-east-1".to_string()),
                        version: None,
                        description: "".to_string(),
                        reference: "".to_string(),
                    },
                },
                tf_variables: Vec::with_capacity(0),
                tf_extra_environment_variables: Vec::with_capacity(0),
                s3_key: "".to_string(),
            },
        ];
        validate_providers(&tf_providers);
    }
}
