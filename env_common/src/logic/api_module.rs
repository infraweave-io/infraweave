use anyhow::Result;
use env_defs::{
    get_module_identifier, CloudProvider, ModuleManifest, ModuleResp, ModuleVersionDiff, TfVariable,
};
use env_utils::{
    contains_terraform_lockfile, generate_module_example_deployment, get_outputs_from_tf_files,
    get_timestamp, get_variables_from_tf_files, merge_json_dicts, read_tf_directory, semver_parse,
    validate_module_schema, validate_tf_backend_not_set, zero_pad_semver,
};
use log::{debug, info};
use std::path::Path;

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
) -> anyhow::Result<(), ModuleError> {
    let module_yaml_path = Path::new(manifest_path).join("module.yaml");
    let manifest =
        std::fs::read_to_string(&module_yaml_path).expect("Failed to read module manifest file");

    let mut module_yaml =
        serde_yaml::from_str::<ModuleManifest>(&manifest).expect("Failed to parse module manifest");

    if version_arg.is_some() {
        // In case a version argument is provided
        if module_yaml.spec.version.is_some() {
            panic!("Version is not allowed when version is already set in module.yaml");
        }
        info!("Using version: {}", version_arg.as_ref().unwrap());
        module_yaml.spec.version = Some(version_arg.unwrap().to_string());
    }

    let zip_file = match env_utils::get_zip_file(Path::new(manifest_path), &module_yaml_path).await
    {
        Ok(zip_file) => zip_file,
        Err(error) => {
            return Err(ModuleError::ZipError(error.to_string()));
        }
    };
    // Encode the zip file content to Base64
    let zip_base64 = base64::encode(&zip_file);

    let tf_content = read_tf_directory(Path::new(manifest_path)).unwrap(); // Get all .tf-files concatenated into a single string

    match validate_tf_backend_not_set(&tf_content) {
        std::result::Result::Ok(_) => (),
        Err(error) => {
            println!("{}", error);
            std::process::exit(1);
        }
    }

    match contains_terraform_lockfile(&zip_file) {
        std::result::Result::Ok(exists) => {
            if !exists {
                return Err(ModuleError::TerraformLockfileMissing);
            }
        }
        Err(error) => {
            return Err(ModuleError::ZipError(error.to_string()));
        }
    }

    match validate_module_schema(&manifest) {
        std::result::Result::Ok(_) => (),
        Err(error) => {
            return Err(ModuleError::InvalidModuleSchema(error.to_string()));
        }
    }

    let tf_variables = get_variables_from_tf_files(&tf_content).unwrap();
    let tf_outputs = get_outputs_from_tf_files(&tf_content).unwrap();

    let module = module_yaml.metadata.name.clone();
    let version = module_yaml.spec.version.clone().unwrap();

    let manifest_version = semver_parse(&version).unwrap();
    ensure_track_matches_version(track, &version)?;

    if let Some(ref mut examples) = module_yaml.spec.examples {
        for example in examples.iter() {
            let example_variables = &example.variables;
            let (is_valid, error) =
                is_all_module_example_variables_valid(&tf_variables, example_variables);
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

    let version_diff = match latest_version {
        // TODO break out to function
        Some(previous_existing_module) => {
            let current_version_module_hcl_str = &tf_content;

            // Download the previous version of the module and get hcl content
            let previous_version_s3_key = &previous_existing_module.s3_key;
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

    let module = ModuleResp {
        track: track.to_string(),
        track_version: format!(
            "{}#{}",
            track,
            zero_pad_semver(version.as_str(), 3).unwrap()
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
        s3_key: format!(
            "{}/{}-{}.zip",
            &module_yaml.metadata.name, &module_yaml.metadata.name, &version
        ), // s3_key -> "{module}/{module}-{version}.zip"
        stack_data: None,
        version_diff,
        cpu: module_yaml.spec.cpu.unwrap_or_else(get_default_cpu),
        memory: module_yaml.spec.memory.unwrap_or_else(get_default_memory),
    };

    let all_regions = handler.get_all_regions().await.unwrap();
    info!("Publishing module in all regions: {:?}", all_regions);
    for region in all_regions {
        let region_handler = handler.copy_with_region(&region).await;
        match upload_module(&region_handler, &module, &zip_base64).await {
            Ok(_) => {
                info!("Module published successfully in region {}", region);
            }
            Err(error) => {
                return Err(ModuleError::UploadModuleError(error.to_string()));
            }
        }
    }

    info!("Module published successfully in all regions!");
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
            info!("Successfully published module {}", module.module);
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
        "SK": format!("VERSION#{}", zero_pad_semver(&module.version, 3).unwrap()),
    }))
    .unwrap();

    let module_value = serde_json::to_value(module).unwrap();
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
    }))
    .unwrap();

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

pub async fn compare_latest_version(
    handler: &GenericCloudHandler,
    module: &str,
    version: &str,
    track: &str,
    module_type: ModuleType,
) -> Result<Option<ModuleResp>, anyhow::Error> {
    if version.starts_with("0.0.0") {
        info!("Skipping version check for unreleased version {}", version);
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
                let manifest_version = env_utils::semver_parse(version).unwrap();
                let latest_version = env_utils::semver_parse(&latest_module.version).unwrap();

                // Since semver crate breaks the semver spec (to follow cargo-variant) by also comparing build numbers, we need to compare without build
                // https://github.com/dtolnay/semver/issues/172
                let manifest_version_no_build =
                    env_utils::semver_parse_without_build(version).unwrap();
                let latest_version_no_build =
                    env_utils::semver_parse_without_build(&latest_module.version).unwrap();

                debug!("manifest_version: {:?}", manifest_version);
                debug!("latest_version: {:?}", latest_version);

                if manifest_version_no_build == latest_version_no_build {
                    if manifest_version.build == latest_version.build {
                        return Err(anyhow::anyhow!(
                            "{} version {} already exists in track {}",
                            entity,
                            manifest_version,
                            track
                        ));
                    }
                    info!(
                        "Newer build version of same version {} => {}",
                        latest_version.build, manifest_version.build
                    );
                    Ok(Some(latest_module))
                } else if manifest_version_no_build < latest_version_no_build {
                    return Err(anyhow::anyhow!(
                        "{} version {} is older than the latest version {} in track {}",
                        entity,
                        manifest_version,
                        latest_version,
                        track
                    ));
                } else {
                    info!(
                        "{} version {} is confirmed to be the newest version",
                        entity, manifest_version
                    );
                    return Ok(Some(latest_module));
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

pub async fn download_module_to_vec(handler: &GenericCloudHandler, s3_key: &String) -> Vec<u8> {
    info!("Downloading module from {}...", s3_key);

    let url = match get_module_download_url(handler, s3_key).await {
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

pub async fn get_module_download_url(
    handler: &GenericCloudHandler,
    key: &str,
) -> Result<String, anyhow::Error> {
    let url = match handler.generate_presigned_url(key).await {
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
            let claim_str = serde_yaml::to_string(&example_claim).unwrap();
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
            let error = format!("Example variable {} is not snake_case", key_str);
            return (false, error); // Example-variable is not snake_case
        }
        let tf_variable = tf_variables.iter().find(|&x| x.name == key_str);
        if tf_variable.is_none() {
            let error = format!("Example variable {} does not exist", key_str);
            return (false, error); // Example-variable does not exist
        }
        let tf_variable = tf_variable.unwrap();
        let is_nullable = tf_variable.nullable.unwrap_or(false);
        if tf_variable.default.is_none() && !is_nullable && value.is_null() {
            let error = format!("Required variable {} is null but mandatory", key_str);
            return (false, error); // Required variable is null
        }
    }
    // Check that all required variables are present in example_variables
    for tf_variable in tf_variables.iter() {
        let is_nullable = tf_variable.nullable.unwrap_or(false);
        if tf_variable.default.is_none() && !is_nullable {
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

fn convert_module_example_variables_to_camel_case(
    variables: &serde_yaml::Value,
) -> serde_yaml::Value {
    let variables = to_mapping(variables.clone()).unwrap();
    let mut converted_variables = serde_yaml::Mapping::new();
    for (key, value) in variables.iter() {
        let key_str = key.as_str().unwrap();
        let camel_case_key = env_utils::to_camel_case(key_str);
        converted_variables.insert(
            serde_yaml::Value::String(camel_case_key.to_string()),
            value.clone(),
        );
    }
    serde_yaml::to_value(converted_variables).unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn test_convert_module_example_variables_to_camel_case() {
        let variables = serde_yaml::from_str::<serde_yaml::Value>(
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
        let camel_case_example = convert_module_example_variables_to_camel_case(&variables);
        let expected_camel_case_example = r#"---
bucketName: some-bucket-name
tags:
  oneTag: value1
  anotherTag: value2
portMapping:
  - containerPort: 80
    hostPort: 80
"#;
        assert_eq!(
            serde_yaml::to_string(&camel_case_example).unwrap(),
            expected_camel_case_example
        );
    }

    #[test]
    fn test_is_example_variables_valid() {
        let tf_variables = vec![
            TfVariable {
                name: "bucket_name".to_string(),
                description: Some("The name of the bucket".to_string()),
                default: None,
                sensitive: Some(false),
                nullable: Some(false),
                _type: serde_json::Value::String("string".to_string()),
            },
            TfVariable {
                name: "tags".to_string(),
                description: Some("The tags to apply to the bucket".to_string()),
                default: None,
                sensitive: Some(false),
                nullable: Some(false),
                _type: serde_json::Value::String("map".to_string()),
            },
            TfVariable {
                name: "port_mapping".to_string(),
                description: Some("The port mapping".to_string()),
                default: None,
                sensitive: Some(false),
                nullable: Some(false),
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
                description: Some("Instance name".to_string()),
                default: Some(serde_json::Value::String("my-instance".to_string())),
                sensitive: Some(false),
                nullable: Some(false),
                _type: serde_json::Value::String("string".to_string()),
            },
            TfVariable {
                name: "bucket_name".to_string(),
                description: Some("Bucket name".to_string()),
                default: None,
                sensitive: Some(false),
                nullable: Some(false),
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
                description: Some("Instance name".to_string()),
                default: None,
                sensitive: Some(false),
                nullable: Some(false),
                _type: serde_json::Value::String("string".to_string()),
            },
            TfVariable {
                name: "bucket_name".to_string(),
                description: Some("Bucket name".to_string()),
                default: None,
                sensitive: Some(false),
                nullable: Some(false),
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
                description: Some("Instance name".to_string()),
                default: None,
                sensitive: Some(false),
                nullable: Some(true),
                _type: serde_json::Value::String("string".to_string()),
            },
            TfVariable {
                name: "bucket_name".to_string(),
                description: Some("Bucket name".to_string()),
                default: None,
                sensitive: Some(false),
                nullable: Some(false),
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
            description: Some("The name of the bucket".to_string()),
            default: None,
            sensitive: Some(false),
            nullable: Some(false),
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
            description: Some("Bucket name".to_string()),
            default: None,
            sensitive: Some(false),
            nullable: Some(false),
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
}
