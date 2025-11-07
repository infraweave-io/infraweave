use anyhow::Result;
use base64::engine::general_purpose::STANDARD as base64;
use base64::Engine;
use env_defs::{CloudProvider, ProviderManifest, ProviderResp, TfLockProvider, TfVariable};
use env_utils::{
    get_provider_url_key, get_timestamp, get_variables_from_tf_files, merge_json_dicts,
    read_tf_from_zip, semver_parse, zero_pad_semver,
};
use futures::stream::{self, StreamExt};
use log::{debug, info, warn};
use std::{cmp::Ordering, path::Path};

use crate::{errors::ModuleError, interface::GenericCloudHandler};

pub async fn publish_provider(
    handler: &GenericCloudHandler,
    manifest_path: &str,
    version_arg: Option<&str>,
) -> anyhow::Result<(), ModuleError> {
    let provider_yaml_path = Path::new(manifest_path).join("provider.yaml");
    let manifest = std::fs::read_to_string(&provider_yaml_path)
        .expect("Failed to read provider manifest file");

    let mut provider_yaml = serde_yaml::from_str::<ProviderManifest>(&manifest)
        .expect("Failed to parse provider manifest");

    if version_arg.is_some() {
        // In case a version argument is provided
        if provider_yaml.spec.version.is_some() {
            panic!("Version is not allowed when version is already set in provider.yaml");
        }
        info!("Using version: {}", version_arg.as_ref().unwrap());
        provider_yaml.spec.version = Some(version_arg.unwrap().to_string());
    }

    let zip_file =
        match env_utils::get_zip_file(Path::new(manifest_path), &provider_yaml_path).await {
            Ok(zip_file) => zip_file,
            Err(error) => {
                return Err(ModuleError::ZipError(error.to_string()));
            }
        };

    publish_provider_from_zip(handler, provider_yaml, &zip_file).await
}

pub async fn publish_provider_from_zip(
    handler: &GenericCloudHandler,
    provider_yaml: ProviderManifest,
    zip_file: &[u8],
) -> Result<(), ModuleError> {
    // Encode the zip file content to Base64
    let zip_base64 = base64.encode(&zip_file);

    let tf_content = read_tf_from_zip(&zip_file).unwrap(); // Get all .tf-files concatenated into a single string

    let _ = serde_yaml::to_string(&provider_yaml)
        .expect("Failed to serialize provider manifest to YAML");

    let provider = provider_yaml.metadata.name.clone();
    let version = match provider_yaml.spec.version.clone() {
        Some(version) => version,
        None => {
            return Err(ModuleError::ModuleVersionMissing(
                provider_yaml.metadata.name.clone(),
            ));
        }
    };

    let manifest_version = semver_parse(&version).map_err(|e| anyhow::anyhow!(e))?;

    if !manifest_version.pre.is_empty() || !manifest_version.build.is_empty() {
        return Err(ModuleError::InvalidStableVersion);
    }

    info!(
        "Publishing provider: {}, version \"{}.{}.{}\"",
        provider, manifest_version.major, manifest_version.minor, manifest_version.patch,
    );

    let _latest_version: Option<ProviderResp> =
        match compare_latest_version(handler, &provider, &version).await {
            Ok(existing_version) => existing_version, // Returns existing provider if newer, otherwise it's the first provider version to be published
            Err(error) => {
                // If the provider version already exists and is older, exit
                return Err(ModuleError::ModuleVersionExists(version, error.to_string()));
            }
        };

    let _tf_variables = get_variables_from_tf_files(&tf_content).unwrap();
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

    let provider = ProviderResp {
        version: version.clone(),
        timestamp: get_timestamp(),
        name: provider_yaml.metadata.name.clone(),
        // alias: provider_yaml.spec.alias.clone(),
        description: provider_yaml.spec.description.clone(),
        reference: provider_yaml.spec.reference.clone(),
        manifest: provider_yaml.clone(),
        tf_variables: tf_variables,
        tf_extra_environment_variables: tf_extra_environment_variables,
        s3_key: format!(
            "{}/{}-{}.zip",
            &provider_yaml.metadata.name, &provider_yaml.metadata.name, &version
        ), // s3_key -> "{provider}/{provider}-{version}.zip"
    };

    let all_regions = handler.get_all_regions().await?;

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

    println!(
        "Publishing provider in all regions with concurrency limit: {}",
        effective_concurrency_limit
    );

    // Combine all upload tasks into a single vector using boxed futures
    let mut all_upload_tasks: Vec<
        std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), ModuleError>> + Send>>,
    > = Vec::new();

    // Add provider upload tasks
    for region in all_regions.iter() {
        let handler = handler.clone();
        let region = region.clone();
        let provider_ref = provider.clone();
        let zip_base64_ref = zip_base64.clone();

        let task = Box::pin(async move {
            let region_handler = handler.copy_with_region(&region).await;
            match upload_provider(&region_handler, &provider_ref, &zip_base64_ref).await {
                Ok(_) => {
                    println!(
                        "Provider {} is stored in region {}",
                        provider_ref.name, region
                    );
                    Ok(())
                }
                Err(error) => Err(ModuleError::UploadModuleError(format!(
                    "Failed to upload provider {} to region {}: {}",
                    provider_ref.name, region, error
                ))),
            }
        });
        all_upload_tasks.push(task);
    }

    let concurrency_limit = std::cmp::min(all_upload_tasks.len(), effective_concurrency_limit);
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

    info!("Successfully completed all provider uploads.");

    Ok(())
}

pub async fn upload_provider(
    handler: &GenericCloudHandler,
    provider: &ProviderResp,
    zip_base64: &String,
) -> anyhow::Result<(), anyhow::Error> {
    let payload = serde_json::json!({
        "event": "upload_file_base64",
        "data":
        {
            "key": &provider.s3_key,
            "bucket_name": "modules",
            "base64_content": &zip_base64
        }

    });
    match handler.run_function(&payload).await {
        Ok(_) => {
            info!("Successfully uploaded provider zip file to storage");
        }
        Err(error) => {
            return Err(anyhow::anyhow!("{}", error));
        }
    }

    match insert_provider(handler, provider).await {
        Ok(_) => {
            info!("Successfully published provider {}", provider.name);
        }
        Err(error) => {
            return Err(anyhow::anyhow!("{}", error));
        }
    }

    info!(
        "Publishing version {} of provider {}",
        provider.version, provider.name
    );

    Ok(())
}

pub async fn upload_provider_cache(
    handler: &GenericCloudHandler,
    tf_lock_provider: &TfLockProvider,
) -> anyhow::Result<(), anyhow::Error> {
    let target = "linux_arm64"; // TODO: Make this dynamic, for azure it should be "linux_amd64"
    let categories = ["provider_binary", "shasum", "signature"];

    for category in categories.iter() {
        let (url, key) = get_provider_url_key(tf_lock_provider, target, category).await?;
        let payload = serde_json::json!({
            "event": "upload_file_url",
            "data":
            {
                "key": key,
                "bucket_name": "providers",
                "url": url
            }

        });
        match handler.run_function(&payload).await {
            Ok(response) => {
                if response
                    .payload
                    .get("object_already_exists")
                    .is_some_and(|x| x.as_bool() == Some(true))
                {
                    return Ok(());
                }
                info!(
                    "Successfully ensured {} {} for version {} exists",
                    category.replace("_", " "),
                    tf_lock_provider.source,
                    tf_lock_provider.version
                );
            }
            Err(error) => {
                return Err(anyhow::anyhow!("{}", error));
            }
        }
    }
    Ok(())
}

pub async fn insert_provider(
    handler: &GenericCloudHandler,
    provider: &ProviderResp,
) -> anyhow::Result<String> {
    let provider_table_placeholder = "modules";

    let mut transaction_items = vec![];

    let id: String = format!("PROVIDER#{}", &provider.name);

    // -------------------------
    // Provider metadata
    // -------------------------
    let mut provider_payload = serde_json::to_value(serde_json::json!({
        "PK": id.clone(),
        "SK": format!("VERSION#{}", zero_pad_semver(&provider.version, 3)?),
    }))
    .unwrap();

    let provider_value = serde_json::to_value(provider)?;
    merge_json_dicts(&mut provider_payload, &provider_value);

    transaction_items.push(serde_json::json!({
        "Put": {
            "TableName": provider_table_placeholder,
            "Item": provider_payload
        }
    }));

    // -------------------------
    // Latest provider version
    // -------------------------
    // It is inserted as a PROVIDER (above) but LATEST-prefix is used to differentiate provider (to reduce maintenance)
    let latest_pk = "LATEST_PROVIDER";
    let mut latest_provider_payload = serde_json::to_value(serde_json::json!({
        "PK": latest_pk,
        "SK": id.clone(),
    }))?;

    // Use the same provider metadata to the latest provider version
    merge_json_dicts(&mut latest_provider_payload, &provider_value);

    transaction_items.push(serde_json::json!({
        "Put": {
            "TableName": provider_table_placeholder,
            "Item": latest_provider_payload
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
    provider: &str,
    version: &str,
) -> Result<Option<ProviderResp>, anyhow::Error> {
    if version.starts_with("0.0.0") {
        warn!("Skipping version check for unreleased version {}", version);
        return Ok(None); // Used for unreleased versions (for testing in pipeline)
    }

    let fetch_provider: Result<Option<ProviderResp>, anyhow::Error> =
        handler.get_latest_provider_version(provider).await;

    let entity = "Provider";

    match fetch_provider {
        Ok(fetch_provider) => {
            if let Some(latest_provider) = fetch_provider {
                let manifest_version = env_utils::semver_parse(version)?;
                let latest_version = env_utils::semver_parse(&latest_provider.version)?;

                // Since semver crate breaks the semver spec (to follow cargo-variant) by also comparing build numbers, we need to compare without build
                // https://github.com/dtolnay/semver/issues/172
                let manifest_version_no_build = env_utils::semver_parse_without_build(version)?;
                let latest_version_no_build =
                    env_utils::semver_parse_without_build(&latest_provider.version)?;

                debug!("manifest_version: {:?}", manifest_version);
                debug!("latest_version: {:?}", latest_version);

                match manifest_version_no_build.cmp(&latest_version_no_build) {
                    Ordering::Equal => {
                        // Same version number, check build
                        if manifest_version.build == latest_version.build {
                            Err(anyhow::anyhow!(
                                "{} version {} already exists",
                                entity,
                                manifest_version,
                            ))
                        } else {
                            info!(
                                "Newer build version of same version {} => {}",
                                latest_version.build, manifest_version.build
                            );
                            Ok(Some(latest_provider))
                        }
                    }

                    Ordering::Less => Err(anyhow::anyhow!(
                        "{} version {} is older than the latest version {}",
                        entity,
                        manifest_version,
                        latest_version
                    )),

                    Ordering::Greater => {
                        info!(
                            "{} version {} is confirmed to be the newest version",
                            entity, manifest_version
                        );
                        Ok(Some(latest_provider))
                    }
                }
            } else {
                info!(
                    "No existing {} version found, this is the first version",
                    entity
                );
                Ok(None)
            }
        }
        Err(e) => Err(anyhow::anyhow!("An error occurred: {:?}", e)),
    }
}

pub async fn download_provider_to_vec(handler: &GenericCloudHandler, s3_key: &String) -> Vec<u8> {
    info!("Downloading provider from {}...", s3_key);

    let url = match get_provider_download_url(handler, s3_key).await {
        Ok(url) => url,
        Err(e) => {
            panic!("Error: {:?}", e);
        }
    };

    match env_utils::download_zip_to_vec(&url).await {
        Ok(content) => {
            info!("Downloaded provider");
            content
        }
        Err(e) => {
            panic!("Error: {:?}", e);
        }
    }
}

pub async fn get_provider_download_url(
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
