use anyhow::anyhow;
use env_common::DeploymentStatusHandler;
use env_defs::{ApiInfraPayload, CloudProvider, ModuleResp, OciArtifactSet};
use env_utils::get_module_zip_from_oci_targz;
use log::{error, info};
use std::path::Path;

use env_common::{get_modules_download_url, interface::GenericCloudHandler};

pub async fn download_module_zip(
    handler: &GenericCloudHandler,
    s3_key: &String,
    destination: &str,
) -> Result<(), anyhow::Error> {
    println!("Downloading module zip from {}", s3_key);

    let url = match get_modules_download_url(handler, s3_key).await {
        Ok(url) => url,
        Err(e) => {
            return Err(anyhow::anyhow!("Error: {:?}", e));
        }
    };

    match env_utils::download_zip(&url, Path::new("module.zip")).await {
        Ok(_) => {
            println!("Downloaded module");
        }
        Err(e) => {
            return Err(anyhow::anyhow!("Error: {:?}", e));
        }
    }

    match env_utils::unzip_file(Path::new("module.zip"), Path::new(destination)) {
        Ok(_) => {
            println!("Unzipped module");
        }
        Err(e) => {
            return Err(anyhow::anyhow!("Error: {:?}", e));
        }
    }
    Ok(())
}

pub async fn download_module_oci(
    handler: &GenericCloudHandler,
    oci_artifact_set: &OciArtifactSet,
    destination: &str,
) -> Result<ModuleResp, anyhow::Error> {
    let files: Vec<String> = vec![
        oci_artifact_set.tag_main.clone(),
        oci_artifact_set.tag_signature.as_ref().unwrap().clone(),
        oci_artifact_set.tag_attestation.as_ref().unwrap().clone(),
    ];

    println!("Downloading module oci files: {:?}", files);
    if !Path::new(destination).exists() {
        std::fs::create_dir_all(destination)?;
    }

    for file in files {
        let file_path = format!(
            "{}/{}.tar.gz",
            oci_artifact_set.oci_artifact_path.trim_end_matches("/"),
            file
        );
        println!("Downloading file: {}", file_path);
        let url = match get_modules_download_url(handler, &file_path).await {
            Ok(url) => url,
            Err(e) => {
                return Err(anyhow::anyhow!("Error: {:?}", e));
            }
        };

        let destination_path = format!("{}/{}.tar.gz", destination, file);
        match env_utils::download_zip(&url, Path::new(&destination_path)).await {
            Ok(_) => {
                println!("Downloaded {} to {}", url, destination_path);
            }
            Err(e) => {
                return Err(anyhow::anyhow!("Error: {:?}", e));
            }
        }
    }

    env_utils::verify_oci_artifacts_offline(oci_artifact_set, None)
        .map_err(|e| anyhow::anyhow!("Error verifying OCI artifacts: {:?}", e))?;

    let artifact_path = format!("{}/{}.tar.gz", destination, oci_artifact_set.tag_main);
    let module_zip_bytes = get_module_zip_from_oci_targz(&artifact_path)
        .map_err(|e| anyhow::anyhow!("Error extracting module zip from OCI tar.gz: {:?}", e))?;
    let zip_destination = format!("{}/module.zip", destination);
    println!("Store zip bytes to: {}", zip_destination);
    env_utils::store_zip_bytes(&module_zip_bytes, Path::new(&zip_destination))
        .map_err(|e| anyhow::anyhow!("Error storing zip bytes to {}: {:?}", zip_destination, e))?;

    let unzipped_destination = destination.to_string();

    println!("Unzipping {} to {}", zip_destination, unzipped_destination);
    env_utils::unzip_file(
        Path::new(&zip_destination),
        Path::new(&unzipped_destination),
    )
    .map_err(|e| {
        anyhow::anyhow!(
            "Error unzipping file {} to {}: {:?}",
            zip_destination,
            unzipped_destination,
            e
        )
    })?;

    let module_resp = env_utils::get_module_manifest_from_oci_targz(&artifact_path).unwrap();

    Ok(module_resp)
}

pub async fn get_module(
    handler: &GenericCloudHandler,
    payload: &ApiInfraPayload,
    status_handler: &mut DeploymentStatusHandler<'_>,
) -> Result<env_defs::ModuleResp, anyhow::Error> {
    let track = payload.module_track.clone();
    let is_stack = payload.module_type == "stack";

    match handler
        .get_module_version(&payload.module, &track, &payload.module_version)
        .await
    {
        Ok(module) => {
            info!("Successfully fetched module: {:?}", module);
            if let Some(module) = module {
                // Check if the module is deprecated - allow existing deployments but block new ones
                match env_common::logic::check_module_deprecation(
                    handler,
                    &module,
                    is_stack,
                    &payload.module,
                    &payload.module_version,
                    &payload.deployment_id,
                    &payload.environment,
                )
                .await
                {
                    Ok(_) => {
                        // Module is not deprecated or is deprecated but deployment exists
                        Ok(module)
                    }
                    Err(e) => {
                        // Module is deprecated and cannot be used
                        error!("Module deprecation check failed: {:?}", e);
                        let error_text = e.to_string();
                        let status = "failed_init".to_string();
                        status_handler.set_status(status);
                        status_handler.set_event_duration();
                        status_handler.set_error_text(error_text.clone());
                        status_handler.send_event(handler).await;
                        status_handler.send_deployment(handler).await?;
                        Err(anyhow::anyhow!("{}", error_text))
                    }
                }
            } else {
                let error_text = "Module does not exist";
                println!("{}", error_text);
                let status = "failed_init".to_string();
                status_handler.set_status(status);
                status_handler.set_event_duration();
                status_handler.set_error_text(error_text.to_string());
                status_handler.send_event(handler).await;
                status_handler.send_deployment(handler).await?;
                Err(anyhow::anyhow!("Module does not exist"))
            }
        }
        Err(e) => {
            error!("Failed to get module: {:?}", e);
            let status = "failed_init".to_string();
            let error_text: String = e.to_string();
            status_handler.set_status(status);
            status_handler.set_event_duration();
            status_handler.set_error_text(error_text);
            status_handler.send_event(handler).await;
            status_handler.send_deployment(handler).await?;
            Err(anyhow::anyhow!("Failed to get module"))
        }
    }
}

pub async fn download_module(
    handler: &GenericCloudHandler,
    module_from_db: &ModuleResp,
    status_handler: &mut DeploymentStatusHandler<'_>,
) -> Result<(), anyhow::Error> {
    if std::env::var("OCI_ARTIFACT_MODE").is_ok() {
        println!(
            "OCI Artifact Mode is enabled, downloading OCI artifact and running verifications..."
        );
        let module_oci = match download_module_oci(
            handler,
            &module_from_db.oci_artifact_set.clone().unwrap(),
            "./",
        )
        .await
        {
            Ok(module) => Ok(module),
            Err(e) => {
                println!("Error preparing: {:?}", e);
                let status = "failed_prepare".to_string();
                status_handler.set_status(status);
                status_handler.set_event_duration();
                status_handler.send_event(handler).await;
                status_handler.send_deployment(handler).await?;
                Err(anyhow!("Error running terraform init: {}", e))
            }
        }?;

        if compare_module_integrity(&module_oci, module_from_db, handler, status_handler).await? {
            println!("Passed integrity check: module metadata from OCI registry matches the module in the database");
        } else {
            return Err(anyhow::anyhow!(
            "Integrity error! The module metadata from OCI registry does not match the module in the database: \n{}\n!=\n{}",
            serde_json::to_string_pretty(&module_oci)?,
            serde_json::to_string_pretty(&module_from_db)?,
        ));
        }
    } else {
        println!("OCI Artifact Mode is disabled, downloading module zip file...");
        download_module_zip(handler, &module_from_db.s3_key, "./").await?;
    }
    Ok(())
}

async fn compare_module_integrity(
    module_oci: &ModuleResp,
    module_from_db: &ModuleResp,
    handler: &GenericCloudHandler,
    status_handler: &mut DeploymentStatusHandler<'_>,
) -> Result<bool, anyhow::Error> {
    let mut oci_value = serde_json::to_value(module_oci)?;
    let mut db_value = serde_json::to_value(module_from_db)?;

    let ignored_fields = ["timestamp", "oci_artifact_set", "version_diff"];

    for field in &ignored_fields {
        oci_value.as_object_mut().unwrap().remove(*field);
        db_value.as_object_mut().unwrap().remove(*field);
    }

    match oci_value == db_value {
        true => Ok(true),
        false => {
            println!(
                "Error when checking module integrity; {} != {}",
                serde_json::to_string_pretty(&oci_value).unwrap(),
                serde_json::to_string_pretty(&db_value).unwrap()
            );
            let status = "failed_integrity_check".to_string();
            status_handler.set_status(status);
            status_handler.set_event_duration();
            status_handler.send_event(handler).await;
            status_handler.send_deployment(handler).await?;
            Err(anyhow!("Error when checking module integrity"))
        }
    }
}
