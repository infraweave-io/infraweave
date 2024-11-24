use anyhow::Result;
use env_defs::{get_module_identifier, ModuleManifest, ModuleResp, ModuleVersionDiff};
use env_utils::{
    generate_module_example_deployment, get_outputs_from_tf_files, get_timestamp,
    get_variables_from_tf_files, merge_json_dicts, read_tf_directory, semver_parse,
    validate_module_schema, validate_tf_backend_not_set, zero_pad_semver,
};
use log::{debug, info};
use std::path::Path;

use crate::{
    errors::ModuleError,
    interface::CloudHandler,
    logic::{
        common::handler,
        utils::{ensure_track_matches_version, ModuleType},
    },
};

pub async fn publish_module(
    manifest_path: &String,
    track: &String,
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

    let zip_file = match env_utils::get_zip_file(&Path::new(manifest_path), &module_yaml_path).await
    {
        Ok(zip_file) => zip_file,
        Err(error) => {
            return Err(ModuleError::ZipError(error.to_string()));
        }
    };
    // Encode the zip file content to Base64
    let zip_base64 = base64::encode(&zip_file);

    let tf_content = read_tf_directory(&Path::new(manifest_path)).unwrap(); // Get all .tf-files concatenated into a single string

    match validate_tf_backend_not_set(&tf_content) {
        std::result::Result::Ok(_) => (),
        Err(error) => {
            println!("{}", error);
            std::process::exit(1);
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
        match compare_latest_version(&module, &version, &track, ModuleType::Module).await {
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
            let previous_version_module_zip = download_module_to_vec(previous_version_s3_key).await;

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
                &current_version_module_hcl_str,
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
        track: track.clone(),
        track_version: format!(
            "{}#{}",
            track.clone(),
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
        tf_variables: tf_variables,
        tf_outputs: tf_outputs,
        s3_key: format!(
            "{}/{}-{}.zip",
            &module_yaml.metadata.name, &module_yaml.metadata.name, &version
        ), // s3_key -> "{module}/{module}-{version}.zip"
        stack_data: None,
        version_diff: version_diff,
    };

    match upload_module(&module, &zip_base64).await {
        Ok(_) => {
            info!("Module published successfully");
            Ok(())
        }
        Err(error) => {
            return Err(ModuleError::UploadModuleError(error.to_string()));
        }
    }
}

pub async fn upload_module(
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
    match handler().run_function(&payload).await {
        Ok(_) => {
            info!("Successfully uploaded module zip file to storage");
        }
        Err(error) => {
            return Err(anyhow::anyhow!("{}", error));
        }
    }

    match insert_module(&module).await {
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

pub async fn insert_module(module: &ModuleResp) -> anyhow::Result<String> {
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

    let module_value = serde_json::to_value(&module).unwrap();
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
    match handler().run_function(&payload).await {
        Ok(response) => Ok(response.payload.to_string()),
        Err(e) => Err(e),
    }
}

pub async fn compare_latest_version(
    module: &String,
    version: &String,
    track: &String,
    module_type: ModuleType,
) -> Result<Option<ModuleResp>, anyhow::Error> {
    let fetch_module: Result<Option<ModuleResp>, anyhow::Error> = match module_type {
        ModuleType::Module => handler().get_latest_module_version(module, track).await,
        ModuleType::Stack => handler().get_latest_stack_version(module, track).await,
    };

    let entity = if module_type == ModuleType::Module {
        "Module"
    } else {
        "Stack"
    };

    match fetch_module {
        Ok(fetch_module) => {
            if let Some(latest_module) = fetch_module {
                let manifest_version = env_utils::semver_parse(&version).unwrap();
                let latest_version = env_utils::semver_parse(&latest_module.version).unwrap();

                // Since semver crate breaks the semver spec (to follow cargo-variant) by also comparing build numbers, we need to compare without build
                // https://github.com/dtolnay/semver/issues/172
                let manifest_version_no_build =
                    env_utils::semver_parse_without_build(&version).unwrap();
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
                    return Ok(Some(latest_module));
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
                return Ok(None);
            }
        }
        Err(e) => {
            return Err(anyhow::anyhow!("An error occurred: {:?}", e));
        }
    };
}

pub async fn download_module_to_vec(s3_key: &String) -> Vec<u8> {
    info!("Downloading module from {}...", s3_key);

    let url = match get_module_download_url(s3_key).await {
        Ok(url) => url,
        Err(e) => {
            panic!("Error: {:?}", e);
        }
    };

    let zip_vec = match env_utils::download_zip_to_vec(&url).await {
        Ok(content) => {
            info!("Downloaded module");
            content
        }
        Err(e) => {
            panic!("Error: {:?}", e);
        }
    };

    zip_vec
}

pub async fn get_module_download_url(key: &String) -> Result<String, anyhow::Error> {
    let url = match handler().generate_presigned_url(key).await {
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
            let example_claim = generate_module_example_deployment(module_spec, &example);
            let claim_str = serde_yaml::to_string(&example_claim).unwrap();
            info!("{}", claim_str);
        }
    } else {
        info!("No examples found in module.yaml, consider adding some to guide your users");
    }

    Ok(())
}
